#![forbid(unsafe_code)]
#![allow(clippy::needless_pass_by_value)]
//! Leptos CSR presentation shell for `PincerPDF`.

use leptos::prelude::*;

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

const TOOLS: [ToolDefinition; 8] = [
    ToolDefinition {
        slug: "merge",
        test_id: "tool-nav-merge",
        short_label: "Merge",
        title: "Merge PDF",
        description: "Combine ordered source documents into a single output.",
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
                <span class="tool-state">"Not implemented"</span>
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
                    <span>"Overview"</span>
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
                    <span>"8 planned"</span>
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
                        <strong>"Engine evidence ready"</strong>
                        <small>"QPDF structure · MuPDF render"</small>
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
                            {move || if reduced_motion.get() { "Manual mode on" } else { "Follow system" }}
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
                        <small>"The shell exposes scope without pretending the PDF operation exists."</small>
                    </span>
                </div>
                <span class="lock-label">"Capability gated"</span>
            </div>

            <div class="panel-actions">
                <button type="button" class="primary-action" disabled data-testid="open-files">
                    "Open PDF files"
                </button>
                <span>"Available when its vertical slice passes engine and parity gates."</span>
            </div>
        </section>
    }
}

fn readiness_panel() -> impl IntoView {
    view! {
        <section class="panel readiness-panel" aria-labelledby="readiness-title" data-testid="readiness-card">
            <div class="panel-heading">
                <div>
                    <p class="eyebrow">"System readiness"</p>
                    <h2 id="readiness-title">"Verified foundations"</h2>
                </div>
                <span class="status-pill is-ready">"P3"</span>
            </div>
            <ul class="readiness-list">
                <li>
                    <span class="readiness-icon is-ready" aria-hidden="true">"✓"</span>
                    <span><strong>"Rust foundation"</strong><small>"Pinned 1.97.1 · 13 tests"</small></span>
                </li>
                <li>
                    <span class="readiness-icon is-ready" aria-hidden="true">"✓"</span>
                    <span><strong>"PDF engine probe"</strong><small>"32 measured commands"</small></span>
                </li>
                <li>
                    <span class="readiness-icon is-ready" aria-hidden="true">"✓"</span>
                    <span><strong>"Linux environment"</strong><small>"Reproducible devcontainer"</small></span>
                </li>
                <li>
                    <span class="readiness-icon is-ready" aria-hidden="true">"✓"</span>
                    <span><strong>"Application shell"</strong><small>"Leptos CSR + Tauri 2 · 5 E2E"</small></span>
                </li>
            </ul>
        </section>
    }
}

fn activity_panel() -> impl IntoView {
    view! {
        <section class="panel activity-panel" aria-labelledby="activity-title">
            <div class="panel-heading">
                <div>
                    <p class="eyebrow">"Recent activity"</p>
                    <h2 id="activity-title">"No documents yet"</h2>
                </div>
                <span class="status-pill">"Local only"</span>
            </div>
            <div class="empty-state" data-testid="activity-empty-state">
                <span class="empty-icon" aria-hidden="true">"PDF"</span>
                <div>
                    <strong>"Your completed tasks will appear here."</strong>
                    <p>"PincerPDF will not create history until a real PDF tool is enabled."</p>
                </div>
            </div>
        </section>
    }
}

fn architecture_panel() -> impl IntoView {
    view! {
        <section class="panel architecture-panel" aria-labelledby="architecture-title">
            <div class="panel-heading">
                <div>
                    <p class="eyebrow">"Execution model"</p>
                    <h2 id="architecture-title">"Evidence before capability"</h2>
                </div>
            </div>
            <div class="architecture-flow" role="list" aria-label="PDF task execution path">
                <div role="listitem"><span>"01"</span><strong>"Validate input"</strong><small>"Domain rules"</small></div>
                <div role="listitem"><span>"02"</span><strong>"Plan output"</strong><small>"Atomic paths"</small></div>
                <div role="listitem"><span>"03"</span><strong>"Run adapter"</strong><small>"QPDF / MuPDF"</small></div>
                <div role="listitem"><span>"04"</span><strong>"Verify result"</strong><small>"Structure + render"</small></div>
            </div>
        </section>
    }
}

fn workspace(active_tool: ReadSignal<ToolDefinition>) -> impl IntoView {
    view! {
        <div class="workspace">
            <header class="topbar">
                <div>
                    <span class="breadcrumb">"PincerPDF / Overview"</span>
                    <strong>"Application shell checkpoint"</strong>
                </div>
                <div class="topbar-status" role="status">
                    <span class="status-dot is-ready" aria-hidden="true"></span>
                    <span>"Local-first · no document loaded"</span>
                </div>
            </header>

            <main id="main-content" tabindex="-1">
                <section class="hero" aria-labelledby="page-title" data-testid="hero">
                    <div>
                        <p class="eyebrow">"P3 · Application shell"</p>
                        <h1 id="page-title">"A precise workspace for everyday PDF operations."</h1>
                        <p>
                            "The interface, accessibility contract, and native host are real. "
                            "PDF actions remain visibly gated until their vertical slices pass."
                        </p>
                    </div>
                    <div class="hero-metric" aria-label="Current delivery status">
                        <strong>"3 / 11"</strong>
                        <span>"phases established"</span>
                    </div>
                </section>

                <div class="dashboard-grid">
                    {selected_tool_panel(active_tool)}
                    {readiness_panel()}
                    {activity_panel()}
                    {architecture_panel()}
                </div>
            </main>

            <footer class="statusbar" aria-label="Application status">
                <span><strong>"Host"</strong> "Tauri 2"</span>
                <span><strong>"UI"</strong> "Leptos CSR"</span>
                <span><strong>"Motion"</strong> "System + manual control"</span>
                <span class="statusbar-end">"No network services configured"</span>
            </footer>
        </div>
    }
}

#[component]
fn App() -> impl IntoView {
    let (active_tool, set_active_tool) = signal(TOOLS[0]);
    let (reduced_motion, set_reduced_motion) = signal(false);

    view! {
        <a class="skip-link" href="#main-content">"Skip to main content"</a>
        <div
            class=move || {
                if reduced_motion.get() {
                    "app-shell motion-reduced"
                } else {
                    "app-shell"
                }
            }
            data-testid="app-shell"
        >
            {sidebar(active_tool, set_active_tool, reduced_motion, set_reduced_motion)}
            {workspace(active_tool)}
        </div>
    }
}

fn main() {
    leptos::mount::mount_to_body(|| view! { <App /> });
}
