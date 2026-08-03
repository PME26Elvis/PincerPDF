#![forbid(unsafe_code)]
#![allow(clippy::needless_pass_by_value)]
//! Leptos CSR presentation layer for `PincerPDF`.

mod merge_workspace;
mod native_bridge;
mod split_workspace;

use leptos::prelude::*;
use merge_workspace::MergeWorkspace;
use native_bridge::{call_without_args, is_tauri};
use pincerpdf_desktop_api::MergeEngineStatus;
use split_workspace::SplitWorkspace;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ToolDefinition {
    slug: &'static str,
    test_id: &'static str,
    short_label: &'static str,
    title: &'static str,
    description: &'static str,
    icon: &'static str,
    phase: &'static str,
}

impl ToolDefinition {
    fn is_merge(self) -> bool {
        self.slug == "merge"
    }

    fn is_split(self) -> bool {
        self.slug == "split"
    }

    fn state_label(self) -> &'static str {
        if self.is_merge() || self.is_split() {
            "Available"
        } else {
            "Not implemented"
        }
    }
}

const TOOLS: [ToolDefinition; 8] = [
    ToolDefinition {
        slug: "merge",
        test_id: "tool-nav-merge",
        short_label: "Merge",
        title: "Merge PDF",
        description: "Combine ordered source documents into a single verified output.",
        icon: "MG",
        phase: "P4",
    },
    ToolDefinition {
        slug: "split",
        test_id: "tool-nav-split",
        short_label: "Split",
        title: "Split PDF",
        description: "Create predictable page groups without loss or duplication.",
        icon: "SP",
        phase: "P5",
    },
    ToolDefinition {
        slug: "split-bookmarks",
        test_id: "tool-nav-split-bookmarks",
        short_label: "Bookmarks",
        title: "Split by bookmarks",
        description: "Build outputs from validated outline levels and destinations.",
        icon: "BM",
        phase: "P5",
    },
    ToolDefinition {
        slug: "split-size",
        test_id: "tool-nav-split-size",
        short_label: "File size",
        title: "Split by size",
        description: "Partition a document against a target output-size policy.",
        icon: "SZ",
        phase: "P5",
    },
    ToolDefinition {
        slug: "alternate-mix",
        test_id: "tool-nav-alternate-mix",
        short_label: "Alternate mix",
        title: "Alternate mix",
        description: "Interleave pages from two documents with explicit direction.",
        icon: "AM",
        phase: "P6",
    },
    ToolDefinition {
        slug: "insert-pages",
        test_id: "tool-nav-insert-pages",
        short_label: "Insert",
        title: "Insert pages",
        description: "Place one document into another at a controlled page boundary.",
        icon: "IN",
        phase: "P6",
    },
    ToolDefinition {
        slug: "extract",
        test_id: "tool-nav-extract",
        short_label: "Extract",
        title: "Extract pages",
        description: "Select and export pages while preserving their requested order.",
        icon: "EX",
        phase: "P6",
    },
    ToolDefinition {
        slug: "rotate",
        test_id: "tool-nav-rotate",
        short_label: "Rotate",
        title: "Rotate PDF",
        description: "Apply deterministic page rotations with visual verification.",
        icon: "RT",
        phase: "P6",
    },
];

fn tool_button(
    tool: ToolDefinition,
    active_tool: ReadSignal<ToolDefinition>,
    set_active_tool: WriteSignal<ToolDefinition>,
) -> impl IntoView {
    view! {
        <button
            type="button"
            class=move || {
                if active_tool.get() == tool {
                    "tool-nav is-active"
                } else {
                    "tool-nav"
                }
            }
            aria-current=move || (active_tool.get() == tool).then_some("page")
            data-testid=tool.test_id
            on:click=move |_| set_active_tool.set(tool)
        >
            <span class="tool-icon" aria-hidden="true">{tool.icon}</span>
            <span class="tool-copy">
                <span class="tool-name">{tool.short_label}</span>
                <span class=if tool.is_merge() || tool.is_split() { "tool-state is-ready" } else { "tool-state" }>
                    {tool.state_label()}
                </span>
            </span>
        </button>
    }
}

fn sidebar(
    active_tool: ReadSignal<ToolDefinition>,
    set_active_tool: WriteSignal<ToolDefinition>,
    reduced_motion: ReadSignal<bool>,
    set_reduced_motion: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <aside class="sidebar" aria-label="Primary navigation" data-testid="sidebar">
            <div class="brand-lockup">
                <span class="brand-mark" aria-hidden="true">"P"</span>
                <span>
                    <strong>"PincerPDF"</strong>
                    <small>"Native PDF workbench"</small>
                </span>
            </div>

            <nav class="primary-nav" aria-label="Workspace">
                <p class="nav-label">"Workspace"</p>
                <a class="nav-link is-active" href="#main-content" aria-current="page">
                    <span aria-hidden="true">"⌂"</span>
                    <span>"Tools"</span>
                </a>
                <span class="nav-link is-disabled" aria-disabled="true">
                    <span aria-hidden="true">"◷"</span>
                    <span>"Activity"</span>
                    <small>"Later"</small>
                </span>
            </nav>

            <nav class="tool-list" aria-label="PDF tools">
                <div class="nav-heading">
                    <p class="nav-label">"PDF tools"</p>
                    <span>"2 available · 6 planned"</span>
                </div>
                {TOOLS
                    .into_iter()
                    .map(|tool| tool_button(tool, active_tool, set_active_tool))
                    .collect_view()}
            </nav>

            <div class="sidebar-footer">
                <div class="engine-note">
                    <span class="status-dot is-ready" aria-hidden="true"></span>
                    <span>
                        <strong>"Local engine boundary"</strong>
                        <small>"Opaque paths · verified output"</small>
                    </span>
                </div>
                <button
                    type="button"
                    class="motion-toggle"
                    data-testid="motion-toggle"
                    aria-pressed=move || reduced_motion.get().to_string()
                    on:click=move |_| set_reduced_motion.update(|value| *value = !*value)
                >
                    <span aria-hidden="true">"↯"</span>
                    <span>
                        <strong>"Reduce motion"</strong>
                        <small data-testid="motion-mode">
                            {move || if reduced_motion.get() {
                                "Manual mode on"
                            } else {
                                "Follow system"
                            }}
                        </small>
                    </span>
                </button>
            </div>
        </aside>
    }
}

fn selected_tool_panel(active_tool: ReadSignal<ToolDefinition>) -> impl IntoView {
    view! {
        <section class="panel selected-tool" aria-labelledby="selected-tool-title">
            <div class="panel-heading">
                <div>
                    <p class="eyebrow">"Selected workspace"</p>
                    <h2 id="selected-tool-title" data-testid="selected-tool-title">
                        {move || active_tool.get().title}
                    </h2>
                </div>
                <span class="status-pill is-planned" data-testid="selected-tool-status">
                    "Not implemented"
                </span>
            </div>
            <p class="panel-description" data-testid="selected-tool-description">
                {move || active_tool.get().description}
            </p>
            <div class="phase-banner">
                <div>
                    <span class="phase-code">{move || active_tool.get().phase}</span>
                    <span>
                        <strong>"Implementation checkpoint"</strong>
                        <small>"This workspace stays locked until its own parity evidence passes."</small>
                    </span>
                </div>
                <span class="lock-label">"Capability gated"</span>
            </div>
            <div class="panel-actions">
                <button type="button" class="primary-action" disabled data-testid="open-files">
                    "Open PDF files"
                </button>
                <span>"Merge and Split are available now; the remaining tools retain independent gates."</span>
            </div>
        </section>
    }
}

fn readiness_panel() -> impl IntoView {
    view! {
        <section class="panel readiness-panel" aria-labelledby="readiness-title">
            <div class="panel-heading">
                <div>
                    <p class="eyebrow">"System readiness"</p>
                    <h2 id="readiness-title">"Verified foundations"</h2>
                </div>
                <span class="status-pill is-ready">"P4"</span>
            </div>
            <ul class="readiness-list">
                <li><span class="readiness-icon is-ready">"✓"</span><span><strong>"Rust foundation"</strong><small>"Windows local quality gate"</small></span></li>
                <li><span class="readiness-icon is-ready">"✓"</span><span><strong>"Merge core"</strong><small>"QPDF contract · atomic output"</small></span></li>
                <li><span class="readiness-icon is-ready">"✓"</span><span><strong>"Browser adapter"</strong><small>"Deterministic E2E state"</small></span></li>
                <li><span class="readiness-icon is-ready">"✓"</span><span><strong>"Linux compatibility"</strong><small>"Pinned milestone oracle"</small></span></li>
            </ul>
        </section>
    }
}

fn gated_workspace(active_tool: ReadSignal<ToolDefinition>) -> impl IntoView {
    view! {
        <section class="hero compact-hero" aria-labelledby="page-title" data-testid="hero">
            <div>
                <p class="eyebrow">"Future workspace"</p>
                <h1 id="page-title">"Planned with evidence, not placeholders."</h1>
                <p>"Each PDF operation unlocks only after its domain, engine, E2E and visual gates pass."</p>
            </div>
            <div class="hero-metric"><strong>"2 / 8"</strong><span>"tools available"</span></div>
        </section>
        <div class="dashboard-grid">
            {selected_tool_panel(active_tool)}
            {readiness_panel()}
        </div>
    }
}

fn workspace(
    active_tool: ReadSignal<ToolDefinition>,
    engine_status: ReadSignal<MergeEngineStatus>,
) -> impl IntoView {
    view! {
        <div class="workspace">
            <header class="topbar">
                <div>
                    <span class="breadcrumb">
                        {move || format!("PincerPDF / {}", active_tool.get().short_label)}
                    </span>
                    <strong>{move || active_tool.get().title}</strong>
                </div>
                <div class="topbar-status" role="status">
                    <span class="status-dot is-ready" aria-hidden="true"></span>
                    <span>"Windows-first · local document processing"</span>
                </div>
            </header>

            <main id="main-content" tabindex="-1">
                <Show
                    when=move || active_tool.get().is_merge()
                    fallback=move || view! {
                        <Show
                            when=move || active_tool.get().is_split()
                            fallback=move || gated_workspace(active_tool)
                        >
                            <SplitWorkspace engine_status=engine_status />
                        </Show>
                    }
                >
                    <MergeWorkspace engine_status=engine_status />
                </Show>
            </main>

            <footer class="statusbar" aria-label="Application status">
                <span><strong>"Host"</strong> "Tauri 2"</span>
                <span><strong>"UI"</strong> "Leptos CSR"</span>
                <span><strong>"Motion"</strong> "System + manual control"</span>
                <span class="statusbar-end">"No PDF data leaves this device"</span>
            </footer>
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let (active_tool, set_active_tool) = signal(TOOLS[0]);
    let (reduced_motion, set_reduced_motion) = signal(false);
    let native = is_tauri();
    let initial_engine_status = if native {
        MergeEngineStatus {
            ready: false,
            engine_id: None,
            engine_version: None,
            issue: None,
        }
    } else {
        MergeEngineStatus {
            ready: true,
            engine_id: Some("deterministic-browser-adapter".to_owned()),
            engine_version: Some("Browser verification mode".to_owned()),
            issue: None,
        }
    };
    let (engine_status, set_engine_status) = signal(initial_engine_status);
    if native {
        leptos::task::spawn_local(async move {
            match call_without_args::<MergeEngineStatus>("merge_engine_status").await {
                Ok(status) => set_engine_status.set(status),
                Err(error) => set_engine_status.set(MergeEngineStatus {
                    ready: false,
                    engine_id: None,
                    engine_version: None,
                    issue: Some(error),
                }),
            }
        });
    }

    view! {
        <a class="skip-link" href="#main-content">"Skip to main content"</a>
        <div
            class=move || if reduced_motion.get() {
                "app-shell motion-reduced"
            } else {
                "app-shell"
            }
            data-testid="app-shell"
        >
            {sidebar(active_tool, set_active_tool, reduced_motion, set_reduced_motion)}
            {workspace(active_tool, engine_status)}
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(|| view! { <App /> });
}
