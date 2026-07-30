# Authoring Balance Prototype

Throwaway prototypes for **Prototype the Code and TUI Authoring Balance**.

## Revised opportunity prototype

After the first artifact called the code-first premise into question, research narrowed the first advantage to inspectable variation and automation. The revised artifact compares the same musical task through three different language boundaries:

- **A - UI proposal:** the agent emits typed musical operations; source is audit detail only.
- **B - Editable rule:** the agent emits a compact, structured procedural rule editable without general source work.
- **C - Source led:** the agent edits language directly while synchronized overlays explain the result.

Open <http://localhost:4173/variation-workflow.html?variant=A>.

## First workspace prototype

It compares three workspace models on one representative song state:

- **A - Source spine:** source is always the largest surface; timeline, inspector, mixer, and automation are subordinate lenses.
- **B - Task workspaces:** Arrange, Mix, and Automate are full workspaces; a source-impact rail remains visible beside every structured edit.
- **C - Object lens:** navigation starts from project objects and commands; source appears as a focused definition rather than a permanent editor.

Run from this directory:

```sh
python3 -m http.server 4173
```

Open <http://localhost:4173/prototype.html?variant=A>. Use the bottom switcher or left/right arrows to compare variants. The workspace controls are illustrative and update the visible selection, source impact, and staged edit summary in memory only.

This prototype is intentionally disposable. Keep the decision recorded in the ticket, then delete this directory.
