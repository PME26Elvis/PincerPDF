#![forbid(unsafe_code)]
#![allow(clippy::module_name_repetitions)]
//! Process-isolated QPDF adapter for proven `PincerPDF` capabilities.

use pincerpdf_domain::{ErrorCode, PageNumber};
use pincerpdf_engine_api::{
    CapabilitySet, EngineError, EngineIdentity, InspectOptions, PdfCapability, PdfEnginePort,
    PdfMetadata,
};
use pincerpdf_merge::{
    BookmarkPolicy, CancellationToken, CommandEvidence, ExecutionControl, MergeEngineInput,
    MergeEnginePort, MergeEngineRequest, MergeEngineResult, MergeTocPolicy, SecretString,
};
use serde_json::{Map, Value, json};
use std::ffi::OsString;
use std::fmt::Write as _;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

static NEXT_TEMP_ID: AtomicU64 = AtomicU64::new(1);

/// Configuration for external QPDF execution.
#[derive(Clone, Debug)]
pub struct QpdfConfig {
    /// QPDF executable name or path.
    pub executable: PathBuf,
    /// Limits used for discovery and inspection commands.
    pub inspection_control: ExecutionControl,
}

impl Default for QpdfConfig {
    fn default() -> Self {
        Self {
            executable: PathBuf::from("qpdf"),
            inspection_control: ExecutionControl::new(
                Duration::from_secs(30),
                64 * 1024,
                CancellationToken::default(),
            ),
        }
    }
}

/// QPDF-backed inspection and Merge adapter.
#[derive(Clone, Debug)]
pub struct QpdfAdapter {
    config: QpdfConfig,
    identity: EngineIdentity,
}

impl QpdfAdapter {
    /// Discovers the default `qpdf` executable and records its exact version.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when QPDF cannot be started or queried.
    pub fn discover() -> Result<Self, EngineError> {
        Self::from_config(QpdfConfig::default())
    }

    /// Creates an adapter from an explicit executable and inspection policy.
    ///
    /// # Errors
    ///
    /// Returns [`EngineError`] when the executable cannot report a version.
    pub fn from_config(config: QpdfConfig) -> Result<Self, EngineError> {
        let capture = run_process(
            &config.executable,
            &[OsString::from("--version")],
            vec!["--version".to_owned()],
            &config.inspection_control,
        )
        .map_err(|failure| map_process_failure(&failure, false))?;
        let version = capture
            .evidence
            .stdout
            .lines()
            .next()
            .unwrap_or("qpdf version unknown")
            .trim()
            .to_owned();
        Ok(Self {
            config,
            identity: EngineIdentity {
                id: "qpdf-process".to_owned(),
                version,
            },
        })
    }

    fn run_qpdf(
        &self,
        args: &[OsString],
        display_args: Vec<String>,
        control: &ExecutionControl,
        password_supplied: bool,
    ) -> Result<ProcessCapture, EngineError> {
        run_process(&self.config.executable, args, display_args, control)
            .map_err(|failure| map_process_failure(&failure, password_supplied))
    }

    fn prepare_merge_sources(
        &self,
        inputs: &[MergeEngineInput],
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<Vec<PreparedSource>, EngineError> {
        inputs
            .iter()
            .map(|input| {
                if input.pages.is_empty() {
                    return Err(EngineError::new(
                        ErrorCode::InvalidInput,
                        "QPDF merge input resolved to zero pages",
                    ));
                }
                let Some(password) = input.password.as_ref() else {
                    return Ok(PreparedSource {
                        path: input.source.clone(),
                        pages: input.pages.clone(),
                        _password_file: None,
                        _decrypted: None,
                    });
                };
                let password_file = PasswordFile::create(password)
                    .map_err(|error| EngineError::new(ErrorCode::Internal, error.to_string()))?;
                let decrypted = TemporaryPath::new("decrypted-source", "pdf").map_err(|error| {
                    EngineError::new(
                        ErrorCode::Internal,
                        format!("cannot create private decryption directory: {error}"),
                    )
                })?;
                let capture = self.run_qpdf(
                    &[
                        password_file.argument(),
                        OsString::from("--decrypt"),
                        qpdf_path(&input.source),
                        qpdf_path(decrypted.path()),
                    ],
                    vec![
                        "--password-file=<redacted>".to_owned(),
                        "--decrypt".to_owned(),
                        input.source.display().to_string(),
                        decrypted.path().display().to_string(),
                    ],
                    control,
                    true,
                )?;
                evidence.push(capture.evidence);
                Ok(PreparedSource {
                    path: decrypted.path().to_path_buf(),
                    pages: input.pages.clone(),
                    _password_file: Some(password_file),
                    _decrypted: Some(decrypted),
                })
            })
            .collect()
    }

    fn bookmark_plan(
        &self,
        request: &MergeEngineRequest,
        prepared: &[PreparedSource],
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<BookmarkPlan, EngineError> {
        let policy = request.bookmark_policy;
        let inputs = &request.inputs;
        let toc_pages = toc_page_count(request.toc_policy, inputs.len());
        if prepared.len() != inputs.len() {
            return Err(EngineError::new(
                ErrorCode::Internal,
                "prepared Merge sources no longer match the request",
            ));
        }
        let json_control = json_capture_control(control, inputs.len(), inputs);
        let mut output_offset = toc_pages.saturating_add(1);
        let mut roots = Vec::new();
        for (prepared_source, input) in prepared.iter().zip(inputs) {
            let source_roots = if matches!(
                policy,
                BookmarkPolicy::Retain | BookmarkPolicy::RetainAsOneEntryPerDocument
            ) {
                let capture = self.run_qpdf(
                    &[
                        OsString::from("--json=2"),
                        OsString::from("--json-key=outlines"),
                        qpdf_path(&prepared_source.path),
                    ],
                    vec![
                        "--json=2".to_owned(),
                        "--json-key=outlines".to_owned(),
                        prepared_source.path.display().to_string(),
                    ],
                    &json_control,
                    false,
                )?;
                ensure_complete_json(&capture.evidence, "source bookmark tree")?;
                let parsed =
                    parse_source_bookmarks(&capture.evidence.stdout, input, output_offset)?;
                evidence.push(capture.evidence);
                parsed
            } else {
                Vec::new()
            };

            match policy {
                BookmarkPolicy::Discard => {}
                BookmarkPolicy::OneEntryPerDocument => roots.push(BookmarkPlanNode {
                    title: document_bookmark_title(&input.document_title),
                    page_position: Some(output_offset),
                    children: Vec::new(),
                }),
                BookmarkPolicy::Retain => roots.extend(source_roots),
                BookmarkPolicy::RetainAsOneEntryPerDocument => roots.push(BookmarkPlanNode {
                    title: document_bookmark_title(&input.document_title),
                    page_position: Some(output_offset),
                    children: source_roots,
                }),
            }
            let contributed_pages = input.pages.len()
                + usize::from(request.add_blank_page_if_odd && input.pages.len() % 2 == 1);
            output_offset = output_offset
                .checked_add(contributed_pages)
                .ok_or_else(|| {
                    EngineError::new(
                        ErrorCode::InvalidInput,
                        "bookmark page-position plan overflowed",
                    )
                })?;
        }
        Ok(BookmarkPlan { roots })
    }

    fn page_geometry(
        &self,
        source: &Path,
        page: PageNumber,
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<PdfPageGeometry, EngineError> {
        let capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=pages"),
                qpdf_path(source),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=pages".to_owned(),
                source.display().to_string(),
            ],
            &json_capture_control(
                control,
                1,
                &[MergeEngineInput {
                    source: source.to_path_buf(),
                    document_title: String::new(),
                    pages: vec![page],
                    password: None,
                }],
            ),
            false,
        )?;
        ensure_complete_json(&capture.evidence, "source page geometry")?;
        let page_object = parse_page_object_reference(&capture.evidence.stdout, page)?;
        evidence.push(capture.evidence);

        let mut geometry = PdfPageGeometry::default();
        let mut current = Some(page_object);
        for _ in 0..64 {
            let Some((object, generation)) = current else {
                break;
            };
            if generation != 0 {
                return Err(invalid_qpdf_json(
                    "source page object used an unsupported non-zero generation",
                ));
            }
            let option = format!("--show-object={object}");
            let capture = self.run_qpdf(
                &[
                    OsString::from(&option),
                    OsString::from("--"),
                    qpdf_path(source),
                ],
                vec![
                    option.clone(),
                    "--".to_owned(),
                    source.display().to_string(),
                ],
                control,
                false,
            )?;
            ensure_complete_text(&capture.evidence, "source page geometry")?;
            let object_text = capture.evidence.stdout.clone();
            evidence.push(capture.evidence);
            geometry.media_box = geometry
                .media_box
                .or(parse_pdf_number_array(&object_text, "/MediaBox")?);
            geometry.crop_box = geometry
                .crop_box
                .or(parse_pdf_number_array(&object_text, "/CropBox")?);
            geometry.rotate = geometry
                .rotate
                .or(parse_pdf_integer(&object_text, "/Rotate")?);
            current = parse_pdf_reference(&object_text, "/Parent")?;
            if geometry.media_box.is_some() && current.is_none() {
                break;
            }
        }
        let Some(media_box) = geometry.media_box else {
            return Err(invalid_qpdf_json(
                "source page geometry omitted an inherited /MediaBox",
            ));
        };
        Ok(PdfPageGeometry {
            media_box: Some(media_box),
            crop_box: geometry.crop_box,
            rotate: geometry.rotate,
        })
    }

    fn add_bookmark_plan(
        &self,
        input: &Path,
        output: &Path,
        plan: &BookmarkPlan,
        inputs: &[MergeEngineInput],
        control: &ExecutionControl,
        evidence: &mut Vec<CommandEvidence>,
    ) -> Result<usize, EngineError> {
        let json_control = json_capture_control(control, plan.node_count(), inputs);
        let layout_capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=pages"),
                OsString::from("--json-key=qpdf"),
                OsString::from("--json-object=trailer"),
                qpdf_path(input),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=pages".to_owned(),
                "--json-key=qpdf".to_owned(),
                "--json-object=trailer".to_owned(),
                input.display().to_string(),
            ],
            &json_control,
            false,
        )?;
        ensure_complete_json(&layout_capture.evidence, "bookmark page layout")?;
        let layout = parse_outline_layout(&layout_capture.evidence.stdout)?;
        evidence.push(layout_capture.evidence);

        let object_selector = format!(
            "--json-object={},{}",
            layout.catalog_object, layout.catalog_generation
        );
        let catalog_capture = self.run_qpdf(
            &[
                OsString::from("--json=2"),
                OsString::from("--json-key=qpdf"),
                OsString::from(&object_selector),
                qpdf_path(input),
            ],
            vec![
                "--json=2".to_owned(),
                "--json-key=qpdf".to_owned(),
                object_selector,
                input.display().to_string(),
            ],
            control,
            false,
        )?;
        ensure_complete_json(&catalog_capture.evidence, "catalog object")?;
        let catalog = parse_catalog(&catalog_capture.evidence.stdout, &layout.catalog_reference)?;
        evidence.push(catalog_capture.evidence);

        let update = build_bookmark_update(&layout, catalog, &plan.roots)?;
        let update_path = TemporaryPath::new("bookmark-update", "json").map_err(|error| {
            EngineError::new(
                ErrorCode::Internal,
                format!("cannot create private bookmark update: {error}"),
            )
        })?;
        write_json(update_path.path(), &update)?;

        let mut update_argument = OsString::from("--update-from-json=");
        update_argument.push(update_path.path());
        let update_capture = self.run_qpdf(
            &[qpdf_path(input), update_argument, qpdf_path(output)],
            vec![
                input.display().to_string(),
                "--update-from-json=<private-bookmark-plan>".to_owned(),
                output.display().to_string(),
            ],
            control,
      ×­6òÚ$z{-®éÜj×°¢ÖF6‚'V–ÆFW"æ7&VFR‚fF—&V7F÷'’’°¢ö²‚‚’’Óâ°¢ÆWBF‚ÒF—&V7F÷'’æ¦ö–â†f÷&ÖB‚'–ÆöBç¶W‡FVç6–öçÒ"’“°¢&WGW&âö²…6VÆb²F—&V7F÷'’ÂF‚Ò“°¢Ğ¢W'"†W'&÷"’–bW'&÷"æ¶–æB‚’ÓÒ–ó£¤W'&÷$¶–æC£¤Ç&VG”W†—7G2Óâ·Ğ¢W'"†W'&÷"’Óâ&WGW&âW'"†W'&÷"’À¢Ğ¢Ğ¢W'"†–ó£¤W'&÷#£¦æWr€¢–ó£¤W'&÷$¶–æC£¤Ç&VG”W†—7G2À¢&6÷VÆBæ÷BÆÆö6FR&—fFR–æ6W%DbFV×÷&'’F—&V7F÷'’"À¢’¢Ğ ¢fâF‚‚g6VÆb’ÓâeF‚°¢g6VÆbçF€¢Ğ§Ğ ¦–×ÂG&÷f÷"FV×÷&'•F‚°¢fâG&÷‚f×WB6VÆb’°¢ÆWBòÒg3£§&VÖ÷fUöf–ÆR‚g6VÆbçF‚“°¢ÆWBòÒg3£§&VÖ÷fUöF—"‚g6VÆbæF—&V7F÷'’“°¢Ğ§Ğ §7G'V7B&÷VæFVEFW‡B°¢FW‡C¢7G&–ærÀ¢G'Væ6FVC¢&ööÂÀ§Ğ ¢5¶FW&—fR„FV'Vr•Ğ§7G'V7B&ö6W746GW&R°¢Wf–FVæ6S¢6öÖÖæDWf–FVæ6RÀ§Ğ ¢5¶FW&—fR„6ÆöæRÂ6÷’ÂFV'Vr•Ğ¦VçVÒ&ö6W74f–ÇW&T¶–æB°¢7vâÀ¢6æ6VÆÆVBÀ¢F–ÖVD÷WBÀ¢W†—BÀ¢¦ö–âÀ§Ğ ¢5¶FW&—fR„FV'Vr•Ğ§7G'V7B&ö6W74f–ÇW&R°¢¶–æC¢&ö6W74f–ÇW&T¶–æBÀ¢Wf–FVæ6S¢&÷ƒÄ6öÖÖæDWf–FVæ6SâÀ¢ÖW76vS¢7G&–ærÀ§Ğ ¦fâ'Vå÷&ö6W72€¢&öw&Ó¢eF‚À¢&w3¢e´÷57G&–æuÒÀ¢F—7Æ•ö&w3¢fV3Å7G&–æsâÀ¢6öçG&öÃ¢dW†V7WF–öä6öçG&öÂÀ¢’Óâ&W7VÇCÅ&ö6W746GW&RÂ&ö6W74f–ÇW&Sâ°¢ÆWB7F'FVBÒ–ç7FçC£¦æ÷r‚“°¢ÆWB×WB6†–ÆBÒ6öÖÖæC£¦æWr‡&öw&Ò¢æ&w2†&w2¢ç7FF–â…7FF–ó£¦çVÆÂ‚’¢ç7FF÷WB…7FF–ó£§—VB‚’¢ç7FFW'"…7FF–ó£§—VB‚’¢ç7vâ‚¢æÖöW'"‡ÆW'&÷'Â&ö6W74f–ÇW&R°¢¶–æC¢&ö6W74f–ÇW&T¶–æC£¥7vâÀ¢Wf–FVæ6S¢&÷ƒ£¦æWr†V×G•öWf–FVæ6R€¢&öw&ÒÀ¢F—7Æ•ö&w2æ6ÆöæR‚’À¢7F'FVBæVÆ6VB‚’À¢’’À¢ÖW76vS¢f÷&ÖB‚&6ææ÷B7F'BDbVæv–æS¢¶W'&÷'Ò"’À¢Ò“ó° ¢ÆWB7FF÷WBÒ6†–ÆBç7FF÷WBçF¶R‚’æW‡V7B‚'7FF÷WBv2—VB"“°¢ÆWB7FFW'"Ò6†–ÆBç7FFW'"çF¶R‚’æW‡V7B‚'7FFW'"v2—VB"“°¢ÆWBÆ–Ö—BÒ6öçG&öÂæ÷WGWEöÆ–Ö—Eö'—FW2‚“°¢ÆWB7FF÷WE÷&VFW"ÒF‡&VC£§7vâ†Ö÷fRÇÂ&VEö&÷VæFVB‡7FF÷WBÂÆ–Ö—B’“°¢ÆWB7FFW'%÷&VFW"ÒF‡&VC£§7vâ†Ö÷fRÇÂ&VEö&÷VæFVB‡7FFW'"ÂÆ–Ö—B’“° ¢ÆWB×WBf÷&6VEö¶–æBÒæöæS°¢ÆWB7FGW2ÒÆö÷°¢–b6öçG&öÂæ6æ6VÆÆF–öâ‚’æ—5ö6æ6VÆÆVB‚’°¢f÷&6VEö¶–æBÒ6öÖR…&ö6W74f–ÇW&T¶–æC£¤6æ6VÆÆVB“°¢ÆWBòÒ6†–ÆBæ¶–ÆÂ‚“°¢'&V²6†–ÆBçv—B‚“°¢Ğ¢–b7F'FVBæVÆ6VB‚’ãÒ6öçG&öÂçF–ÖV÷WB‚’°¢f÷&6VEö¶–æBÒ6öÖR…&ö6W74f–ÇW&T¶–æC£¥F–ÖVD÷WB“°¢ÆWBòÒ6†–ÆBæ¶–ÆÂ‚“°¢'&V²6†–ÆBçv—B‚“°¢Ğ¢ÖF6‚6†–ÆBçG'•÷v—B‚’°¢ö²…6öÖR‡7FGW2’’Óâ'&V²ö²‡7FGW2’À¢ö²„æöæR’ÓâF‡&VC£§6ÆVW„GW&F–öã£¦g&öÕöÖ–ÆÆ—2ƒ’’À¢W'"†W'&÷"’Óâ'&V²W'"†W'&÷"’À¢Ğ¢Ó° ¢ÆWB7FGW2Ò7FGW2æÖöW'"‡ÆW'&÷'Â&ö6W74f–ÇW&R°¢¶–æC¢&ö6W74f–ÇW&T¶–æC£¤W†—BÀ¢Wf–FVæ6S¢&÷ƒ£¦æWr†V×G•öWf–FVæ6R€¢&öw&ÒÀ¢F—7Æ•ö&w2æ6ÆöæR‚’À¢7F'FVBæVÆ6VB‚’À¢’’À¢ÖW76vS¢f÷&ÖB‚&6ææ÷Bv—Bf÷"DbVæv–æS¢¶W'&÷'Ò"’À¢Ò“ó°¢ÆWB7FF÷WBÒ7FF÷WE÷&VFW"æ¦ö–â‚’æÖöW'"‡Å÷Â&ö6W74f–ÇW&R°¢¶–æC¢&ö6W74f–ÇW&T¶–æC£¤¦ö–âÀ¢Wf–FVæ6S¢&÷ƒ£¦æWr†V×G•öWf–FVæ6R€¢&öw&ÒÀ¢F—7Æ•ö&w2æ6ÆöæR‚’À¢7F'FVBæVÆ6VB‚’À¢’’À¢ÖW76vS¢%DbVæv–æR7FF÷WB&VFW"æ–6¶VB"çFõö÷væVB‚’À¢Ò“ó°¢ÆWB7FFW'"Ò7FFW'%÷&VFW"æ¦ö–â‚’æÖöW'"‡Å÷Â&ö6W74f–ÇW&R°¢¶–æC¢&ö6W74f–ÇW&T¶–æC£¤¦ö–âÀ¢Wf–FVæ6S¢&÷ƒ£¦æWr†V×G•öWf–FVæ6R€¢&öw&ÒÀ¢F—7Æ•ö&w2æ6ÆöæR‚’À¢7F'FVBæVÆ6VB‚’À¢’’À¢ÖW76vS¢%DbVæv–æR7FFW'"&VFW"æ–6¶VB"çFõö÷væVB‚’À¢Ò“ó°¢ÆWBWf–FVæ6RÒWf–FVæ6R€¢&öw&ÒÀ¢F—7Æ•ö&w2À¢7FGW2À¢7F'FVBæVÆ6VB‚’À¢7FF÷WBÀ¢7FFW'"À¢“° ¢–bÆWB6öÖR†¶–æB’Òf÷&6VEö¶–æB°¢ÆWBÖW76vRÒÖF6‚¶–æB°¢&ö6W74f–ÇW&T¶–æC£¤6æ6VÆÆVBÓâ%DbVæv–æR÷W&F–öâv26æ6VÆÆVB"À¢&ö6W74f–ÇW&T¶–æC£¥F–ÖVD÷WBÓâ%DbVæv–æR÷W&F–öâF–ÖVB÷WB"À¢òÓâ%DbVæv–æR÷W&F–öâv2–çFW''WFVB"À¢Ó°¢&WGW&âW'"…&ö6W74f–ÇW&R°¢¶–æBÀ¢Wf–FVæ6S¢&÷ƒ£¦æWr†Wf–FVæ6R’À¢ÖW76vS¢ÖW76vRçFõö÷væVB‚’À¢Ò“°¢Ğ¢–b7FGW2ç7V66W72‚’°¢ö²…&ö6W746GW&R²Wf–FVæ6RÒ¢ÒVÇ6R°¢W'"…&ö6W74f–ÇW&R°¢¶–æC¢&ö6W74f–ÇW&T¶–æC£¤W†—BÀ¢ÖW76vS¢–bWf–FVæ6Rç7FFW'"æ—5öV×G’‚’°¢%DbVæv–æRW†—FVBVç7V66W76gVÆÇ’"çFõö÷væVB‚¢ÒVÇ6R°¢f÷&ÖB‚%DbVæv–æRf–ÆVC¢·Ò"ÂWf–FVæ6Rç7FFW'"¢ÒÀ¢Wf–FVæ6S¢&÷ƒ£¦æWr†Wf–FVæ6R’À¢Ò¢Ğ§Ğ ¦fâ&VEö&÷VæFVB†×WB&VFW#¢–×Â&VBÂÆ–Ö—C¢W6—¦R’Óâ&÷VæFVEFW‡B°¢ÆWB×WB&WF–æVBÒfV3£§v—F…ö66—G’†Æ–Ö—BæÖ–âƒƒ“"’“°¢ÆWB×WBG'Væ6FVBÒfÇ6S°¢ÆWB×WB'VffW"Ò³÷Sƒ²ƒ“%Ó°¢Æö÷°¢ÖF6‚&VFW"ç&VB‚f×WB'VffW"’°¢ö²ƒ’ÂW'"…ò’Óâ'&V²À¢ö²‡&VB’Óâ°¢ÆWB&VÖ–æ–ærÒÆ–Ö—Bç6GW&F–æu÷7V"‡&WF–æVBæÆVâ‚’“°¢ÆWB¶VWÒ&VÖ–æ–æræÖ–â‡&VB“°¢&WF–æVBæW‡FVæEög&öÕ÷6Æ–6R‚f'VffW%²âæ¶VWÒ“°¢G'Væ6FVBÃÒ¶VWÂ&VC°¢Ğ¢Ğ¢Ğ¢&÷VæFVEFW‡B°¢FW‡C¢7G&–æs£¦g&öÕ÷WFc…öÆ÷77’‚g&WF–æVB’çG&–Ò‚’çFõö÷væVB‚’À¢G'Væ6FVBÀ¢Ğ§Ğ ¦fâV×G•öWf–FVæ6R‡&öw&Ó¢eF‚Â&wVÖVçG3¢fV3Å7G&–æsâÂGW&F–öã¢GW&F–öâ’Óâ6öÖÖæDWf–FVæ6R°¢6öÖÖæDWf–FVæ6R°¢&öw&Ó¢&öw&ÒæF—7Æ’‚’çFõ÷7G&–ær‚’À¢&wVÖVçG2À¢W†—Eö6öFS¢æöæRÀ¢GW&F–öåö×3¢GW&F–öâæ5öÖ–ÆÆ—2‚’À¢7FF÷WC¢7G&–æs£¦æWr‚’À¢7FFW'#¢7G&–æs£¦æWr‚’À¢7FF÷WE÷G'Væ6FVC¢fÇ6RÀ¢7FFW'%÷G'Væ6FVC¢fÇ6RÀ¢Ğ§Ğ ¦fâWf–FVæ6R€¢&öw&Ó¢eF‚À¢&wVÖVçG3¢fV3Å7G&–æsâÀ¢7FGW3¢W†—E7FGW2À¢GW&F–öã¢GW&F–öâÀ¢7FF÷WC¢&÷VæFVEFW‡BÀ¢7FFW'#¢&÷VæFVEFW‡BÀ¢’Óâ6öÖÖæDWf–FVæ6R°¢6öÖÖæDWf–FVæ6R°¢&öw&Ó¢&öw&ÒæF—7Æ’‚’çFõ÷7G&–ær‚’À¢&wVÖVçG2À¢W†—Eö6öFS¢7FGW2æ6öFR‚’À¢GW&F–öåö×3¢GW&F–öâæ5öÖ–ÆÆ—2‚’À¢7FF÷WC¢7FF÷WBçFW‡BÀ¢7FFW'#¢7FFW'"çFW‡BÀ¢7FF÷WE÷G'Væ6FVC¢7FF÷WBçG'Væ6FVBÀ¢7FFW'%÷G'Væ6FVC¢7FFW'"çG'Væ6FVBÀ¢Ğ§Ğ ¦fâÖ÷&ö6W75öf–ÇW&R†f–ÇW&S¢e&ö6W74f–ÇW&RÂ77v÷&E÷7WÆ–VC¢&ööÂ’ÓâVæv–æTW'&÷"°¢ÆWB6öÖ&–æVBĞ¢f÷&ÖB‚'·Ò·Ò"Âf–ÇW&RæWf–FVæ6Rç7FF÷WBÂf–ÇW&RæWf–FVæ6Rç7FFW'"’çFõö66–•öÆ÷vW&66R‚“°¢ÆWB6öFRÒÖF6‚f–ÇW&Ræ¶–æB°¢&ö6W74f–ÇW&T¶–æC£¤6æ6VÆÆVBÓâW'&÷$6öFS£¤6æ6VÆÆVBÀ¢&ö6W74f–ÇW&T¶–æC£¥7vâÂ&ö6W74f–ÇW&T¶–æC£¥F–ÖVD÷WBÂ&ö6W74f–ÇW&T¶–æC£¤¦ö–âÓâ°¢W'&÷$6öFS£¤Væv–æTf–ÇW&P¢Ğ¢&ö6W74f–ÇW&T¶–æC£¤W†—@¢–b6öÖ&–æVBæ6öçF–ç2‚&–çfÆ–B77v÷&B"¢ÇÂ6öÖ&–æVBæ6öçF–ç2‚&–æ6÷'&V7B77v÷&B"¢ÇÂ6öÖ&–æVBæ6öçF–ç2‚'77v÷&B—2–æ6÷'&V7B"’Óà¢°¢W'&÷$6öFS£¤–æ6÷'&V7E77v÷&@¢Ğ¢&ö6W74f–ÇW&T¶–æC£¤W†—B–b6öÖ&–æVBæ6öçF–ç2‚'77v÷&B"’bb77v÷&E÷7WÆ–VBÓâ°¢W'&÷$6öFS£¥77v÷&E&WV—&V@¢Ğ¢&ö6W74f–ÇW&T¶–æC£¤W†—BÓâW'&÷$6öFS£¤Væv–æTf–ÇW&RÀ¢Ó°¢ÆWB6öÖÖæBÒf÷&ÖB€¢'·Ò·Ò"À¢f–ÇW&RæWf–FVæ6Rç&öw&ÒÀ¢f–ÇW&RæWf–FVæ6Ræ&wVÖVçG2æ¦ö–â‚""¢“°¢Væv–æTW'&÷#£¦æWr†6öFRÂf÷&ÖB‚'·Ó²6öÖÖæC¢¶6öÖÖæGÒ"Âf–ÇW&RæÖW76vR’§Ğ ¢5¶6fr‡FW7B•Ğ¦ÖöBFW7G2°¢W6R7WW#£¢£°¢5¶6fr‡Væ—‚•Ğ¢W6R–æ6W'FeöÖW&vS£¤6æ6VÆÆF–öåFö¶Vã° ¢5¶6fr‡Væ—‚•Ğ¢5·FW7EĞ¢fâ&÷VæFVEö6GW&UöG&–ç5ö'WE÷&WF–ç5ööæÇ•÷F†Uö6öæf–wW&VEöÆ–Ö—B‚’°¢ÆWB6öçG&öÂĞ¢W†V7WF–öä6öçG&öÃ£¦æWr„GW&F–öã£¦g&öÕ÷6V72ƒ"’ÂRÂ6æ6VÆÆF–öåFö¶Vã£¦FVfVÇB‚’“°¢ÆWB6GW&RÒ'Vå÷&ö6W72€¢Fƒ£¦æWr‚'6‚"’À¢e´÷57G&–æs£¦g&öÒ‚"Ö2"’Â÷57G&–æs£¦g&öÒ‚'&–çFb#3CScsƒ“"•ÒÀ¢fV2²"Ö2"çFõö÷væVB‚’Â'&–çFbÇFW7BÖFFâ"çFõö÷væVB‚•ÒÀ¢f6öçG&öÂÀ¢¢æW‡V7B‚'&ö6W727V66VVG2"“°¢76W'EöW†6GW&RæWf–FVæ6Rç7FF÷WBÂ##3CR"“°¢76W'B†6GW&RæWf–FVæ6Rç7FF÷WE÷G'Væ6FVB“°¢Ğ ¢5¶6fr‡Væ—‚•Ğ¢5·FW7EĞ¢fâF–ÖV÷WEö¶–ÆÇ5÷F†Uö6†–ÆE÷&ö6W72‚’°¢ÆWB6öçG&öÂÒW†V7WF–öä6öçG&öÃ£¦æWr€¢GW&F–öã£¦g&öÕöÖ–ÆÆ—2ƒC’À¢#BÀ¢6æ6VÆÆF–öåFö¶Vã£¦FVfVÇB‚’À¢“°¢ÆWBf–ÇW&RÒ'Vå÷&ö6W72€¢Fƒ£¦æWr‚'6‚"’À¢e´÷57G&–æs£¦g&öÒ‚"Ö2"’Â÷57G&–æs£¦g&öÒ‚'6ÆVW"•ÒÀ¢fV2²"Ö2"çFõö÷væVB‚’Â'6ÆVW"çFõö÷væVB‚•ÒÀ¢f6öçG&öÂÀ¢¢æW‡V7EöW'"‚'&ö6W72×W7BF–ÖR÷WB"“°¢76W'B†ÖF6†W2†f–ÇW&Ræ¶–æBÂ&ö6W74f–ÇW&T¶–æC£¥F–ÖVD÷WB’“°¢Ğ ¢5¶6fr‡Væ—‚•Ğ¢5·FW7EĞ¢fâ6æ6VÆÆF–öåö¶–ÆÇ5÷F†Uö6†–ÆE÷&ö6W72‚’°¢ÆWBFö¶VâÒ6æ6VÆÆF–öåFö¶Vã£¦FVfVÇB‚“°¢ÆWB6æ6VÆÆF–öâÒFö¶Vâæ6ÆöæR‚“°¢ÆWB†æFÆRÒF‡&VC£§7vâ†Ö÷fRÇÂ°¢F‡&VC£§6ÆVW„GW&F–öã£¦g&öÕöÖ–ÆÆ—2ƒC’“°¢6æ6VÆÆF–öâæ6æ6VÂ‚“°¢Ò“°¢ÆWB6öçG&öÂÒW†V7WF–öä6öçG&öÃ£¦æWr„GW&F–öã£¦g&öÕ÷6V72ƒ"’Â#BÂFö¶Vâ“°¢ÆWBf–ÇW&RÒ'Vå÷&ö6W72€¢Fƒ£¦æWr‚'6‚"’À¢e´÷57G&–æs£¦g&öÒ‚"Ö2"’Â÷57G&–æs£¦g&öÒ‚'6ÆVW"•ÒÀ¢fV2²"Ö2"çFõö÷væVB‚’Â'6ÆVW"çFõö÷væVB‚•ÒÀ¢f6öçG&öÂÀ¢¢æW‡V7EöW'"‚'&ö6W72×W7B&R6æ6VÆÆVB"“°¢†æFÆRæ¦ö–â‚’æW‡V7B‚&6æ6VÆÆF–öâF‡&VB¦ö–ç2"“°¢76W'B†ÖF6†W2†f–ÇW&Ræ¶–æBÂ&ö6W74f–ÇW&T¶–æC£¤6æ6VÆÆVB’“°¢Ğ ¢5·FW7EĞ¢fâvU÷7V6–f–6F–öå÷&W6W'fW5ö÷&FW%öæEöGWÆ–6FW2‚’°¢ÆWBvW2Ò°¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'fÆ–B"’À¢vTçVÖ&W#£¦æWrƒ’æW‡V7B‚'fÆ–B"’À¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'fÆ–B"’À¢Ó°¢76W'EöW‡vU÷7V6–f–6F–öâ‚gvW2’Â#2ÃÃ2"“°¢Ğ ¢5·FW7EĞ¢fâvUövVöÖWG'•÷'6W%ö66WG5ö&÷†W5÷&÷FF–öåöæE÷&VçE÷&VfW&Væ6R‚’°¢ÆWBvRÒ#ÃÂõG—RõvRõ&VçB""ôÖVF–&÷‚²ÓÓ#c"s“"Òô7&÷&÷‚²csÒõ&÷FFR#sãâ#°¢76W'EöW€¢'6U÷FeöçVÖ&W%ö'&’‡vRÂ"ôÖVF–&÷‚"’æW‡V7B‚&ÖVF–&÷‚"’À¢6öÖR…°¢"Ó"çFõö÷væVB‚’À¢"Ó#"çFõö÷væVB‚’À¢#c""çFõö÷væVB‚’À¢#s“""çFõö÷væVB‚’À¢Ò¢“°¢76W'EöW€¢'6U÷FeöçVÖ&W%ö'&’‡vRÂ"ô7&÷&÷‚"’æW‡V7B‚&7&÷&÷‚"’À¢6öÖR…°¢#"çFõö÷væVB‚’À¢#"çFõö÷væVB‚’À¢#c"çFõö÷væVB‚’À¢#s"çFõö÷væVB‚’À¢Ò¢“°¢76W'EöW€¢'6U÷Feö–çFVvW"‡vRÂ"õ&÷FFR"’æW‡V7B‚'&÷FF–öâ"’À¢6öÖRƒ#s¢“°¢76W'EöW€¢'6U÷Fe÷&VfW&Væ6R‡vRÂ"õ&VçB"’æW‡V7B‚'&VçB"’À¢6öÖR‚ƒ"Â’¢“°¢Ğ ¢5·FW7EĞ¢fâfö÷FW%÷÷6—F–öå÷&VfW'5÷F†U÷f—6–&ÆUö7&÷ö&÷‚‚’°¢ÆWBvVöÖWG'’ÒFevTvVöÖWG'’°¢ÖVF–ö&÷ƒ¢6öÖR…°¢"Ó"çFõö÷væVB‚’À¢"Ó#"çFõö÷væVB‚’À¢#c""çFõö÷væVB‚’À¢#s“""çFõö÷væVB‚’À¢Ò’À¢7&÷ö&÷ƒ¢6öÖR…°¢##"çFõö÷væVB‚’À¢#3"çFõö÷væVB‚’À¢#Sƒ"çFõö÷væVB‚’À¢#sc"çFõö÷væVB‚’À¢Ò’À¢&÷FFS¢æöæRÀ¢Ó°¢76W'EöW†fö÷FW%÷÷6—F–öâ‚fvVöÖWG'’’ÂƒCBãÂC‚ã’“°¢Ğ ¢5·FW7EĞ¢fâ6÷W&6Uö&öö¶Ö&µ÷Æå÷'VæW5öW†6ÇVFVEöÆVfW5öæEöÖ5÷6VÆV7FVE÷vW2‚’°¢ÆWB–çWBÒÖW&vTVæv–æT–çWB°¢6÷W&6S¢F„'Vc£¦g&öÒ‚&&öö¶Ö&·2çFb"’À¢Fö7VÖVçE÷F—FÆS¢&&öö¶Ö&·2çFb"çFõö÷væVB‚’À¢vW3¢fV2°¢vTçVÖ&W#£¦æWrƒ"’æW‡V7B‚'fÆ–B"’À¢vTçVÖ&W#£¦æWrƒ2’æW‡V7B‚'fÆ–B"’À¢ÒÀ¢77v÷&C¢æöæRÀ¢Ó°¢ÆWB6÷W&6RÒ"2'°¢&÷WFÆ–æW2#¢°¢²'F—FÆR#¢$6†FW""Â&FW7GvW÷6g&öÓ#£Â&¶–G2#¥µ×ÒÀ¢²'F—FÆR#¢$6†FW"""Â&FW7GvW÷6g&öÓ#£"Â&¶–G2#¥°¢²'F—FÆR#¢$VæF—‚"Â&FW7GvW÷6g&öÓ#£2Â&¶–G2#¥µ×Ğ¢×Ğ¢Ğ¢Ò"3° ¢76W'EöW€¢'6U÷6÷W&6Uö&öö¶Ö&·2‡6÷W&6RÂf–çWBÂB’æW‡V7B‚'6÷W&6RÆâ"’À¢fV2´&öö¶Ö&µÆäæöFR°¢F—FÆS¢$6†FW"""çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒB’À¢6†–ÆG&Vã¢fV2´&öö¶Ö&µÆäæöFR°¢F—FÆS¢$VæF—‚"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒR’À¢6†–ÆG&Vã¢fV3£¦æWr‚’À¢ÕÒÀ¢ÕĞ¢“°¢76W'EöW†Fö7VÖVçEö&öö¶Ö&µ÷F—FÆR‚&&öö¶Ö&·2çFb"’Â&&öö¶Ö&·2"“°¢Ğ ¢5·FW7EĞ¢fâ&öö¶Ö&µ÷WFFU÷&W6W'fW5ö6FÆöuöæEöFöW5öæ÷E÷&WÆ6U÷F†U÷G&–ÆW"‚’°¢ÆWBÆ–÷WBÒ÷WFÆ–æTÆ–÷WB°¢ÖWFFF¢§6öâ‡°¢&§6öçfW'6–öâ#¢"À¢'FgfW'6–öâ#¢#ãr"À¢&Ö†ö&¦V7F–B#¢p¢Ò’À¢Ö…öö&¦V7Eö–C¢rÀ¢6FÆöu÷&VfW&Væ6S¢#""çFõö÷væVB‚’À¢6FÆöuöö&¦V7C¢À¢6FÆöuövVæW&F–öã¢À¢vUöö&¦V7G3¢fV2²#2""çFõö÷væVB‚’Â#R""çFõö÷væVB‚’Â#r""çFõö÷væVB‚•ÒÀ¢Ó°¢ÆWB6FÆörÒ§6öâ‡°¢"õvW2#¢#"""À¢"õG—R#¢"ô6FÆör"À¢"ôÆær#¢'S¦Vâ ¢Ò¢æ5öö&¦V7B‚¢æW‡V7B‚&6FÆörö&¦V7B"¢æ6ÆöæR‚“°¢ÆWBW‡V7FVBÒfV2°¢&öö¶Ö&µÆäæöFR°¢F—FÆS¢&öæR"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒ’À¢6†–ÆG&Vã¢fV2´&öö¶Ö&µÆäæöFR°¢F—FÆS¢&6†–ÆB"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒ2’À¢6†–ÆG&Vã¢fV3£¦æWr‚’À¢ÕÒÀ¢ÒÀ¢&öö¶Ö&µÆäæöFR°¢F—FÆS¢'Gvò"çFõö÷væVB‚’À¢vU÷÷6—F–öã¢6öÖRƒ"’À¢6†–ÆG&Vã¢fV3£¦æWr‚’À¢ÒÀ¢Ó° ¢ÆWBWFFRÒ'V–ÆEö&öö¶Ö&µ÷WFFR‚fÆ–÷WBÂ6FÆörÂfW‡V7FVB’æW‡V7B‚&&öö¶Ö&²WFFR"“°¢ÆWBö&¦V7G2ÒWFFU²'Fb%Õ³Òæ5öö&¦V7B‚’æW‡V7B‚'WFFRö&¦V7G2"“° ¢76W'B‚ö&¦V7G2æ6öçF–ç5ö¶W’‚'G&–ÆW""’“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"õvW2%ÒÀ¢fÇVS£¥7G&–ær‚#"""çFõö÷væVB‚’¢“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"ôÆær%ÒÀ¢fÇVS£¥7G&–ær‚'S¦Vâ"çFõö÷væVB‚’¢“°¢76W'EöW†ö&¦V7G5²&ö&££‚"%Õ²'fÇVR%Õ²"ô6÷VçB%ÒÂfÇVS£¦g&öÒƒ"’“°¢76W'EöW€¢ö&¦V7G5²&ö&££’"%Õ²'fÇVR%Õ²"ôFW7B%Õ³ÒÀ¢fÇVS£¥7G&–ær‚#2""çFõö÷væVB‚’¢“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"ôFW7B%Õ³ÒÀ¢fÇVS£¥7G&–ær‚#r""çFõö÷væVB‚’¢“°¢76W'EöW€¢ö&¦V7G5²&ö&££"%Õ²'fÇVR%Õ²"ôFW7B%Õ³ÒÀ¢fÇVS£¥7G&–ær‚#R""çFõö÷væVB‚’¢“°¢Ğ§Ğ 