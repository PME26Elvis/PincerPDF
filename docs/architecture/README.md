# Architecture

PincerPDF uses layered hexagonal architecture:

- **Domain:** pure PDF-tool values, commands, plans, errors and task lifecycle.
- **Application:** use-case orchestration, capability validation, task queue and cancellation.
- **Ports:** PDF engine, filesystem/platform, persistence and inspection interfaces.
- **Adapters:** QPDF/MuPDF candidates, Linux integration, storage and observability.
- **Presentation:** Leptos components and deterministic state adapters.
- **Composition root:** Tauri desktop process with least-privilege commands/capabilities.

Accepted decisions live under `adr/`. Concrete PDF engine types must never cross into domain/application/UI code.
