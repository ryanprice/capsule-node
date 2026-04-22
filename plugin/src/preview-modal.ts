import { App, Modal, Setting, TFile } from "obsidian";
import { runExtraction } from "./capsule-extract";
import { ExtractionError, ExtractionResult } from "./extraction";
import { Manifest } from "./manifest";

const MAX_RECORDS_SHOWN = 20;

/**
 * Preview-only view into what a capsule would expose.
 *
 * Never writes to disk, never talks to the daemon, never encrypts. The
 * whole point is to answer "what am I actually about to share?" before a
 * user flips a capsule to `active`. For frontmatter-list mode we read
 * from Obsidian's metadata cache only; for table and code-fence modes
 * we vault.read each source once.
 */
export class PreviewCapsuleDataModal extends Modal {
	constructor(
		app: App,
		private capsuleFile: TFile,
		private manifest: Manifest,
	) {
		super(app);
	}

	onOpen(): void {
		const { contentEl } = this;
		contentEl.empty();
		contentEl.createEl("h2", { text: `Preview: ${this.manifest.capsule_id}` });
		contentEl.createEl("p", {
			text: `Schema: ${this.manifest.schema} · extraction: ${this.manifest.extraction ?? "none"}`,
			cls: "capsule-preview-subhead",
		});

		const sources = this.manifest.sources ?? [];
		if (sources.length === 0) {
			contentEl.createEl("p", {
				text: 'This capsule has no `sources:` defined. Add one or more wikilinks to the capsule note\'s frontmatter, then re-run "Preview capsule data".',
			});
			this.addCloseButton();
			return;
		}

		const mode = this.manifest.extraction ?? "none";
		if (mode === "none") {
			this.renderNoneHint(sources.length);
			this.addCloseButton();
			return;
		}

		// Table + code-fence require reading each source's full body —
		// that's async. Show a placeholder synchronously, then replace
		// when the resolve+extract pass finishes.
		const status = contentEl.createEl("p", { text: "Loading…" });
		void runExtraction(this.app, this.capsuleFile, this.manifest).then(
			(result) => {
				status.remove();
				this.renderSummary(contentEl, result, sources.length);
				if (result.errors.length > 0) this.renderErrors(contentEl, result.errors);
				if (result.records.length > 0) this.renderRecords(contentEl, result.records);
				this.addCloseButton();
			},
		);
	}

	onClose(): void {
		this.contentEl.empty();
	}

	private renderNoneHint(sourceCount: number): void {
		// The common trip-up: user added sources via the Properties panel,
		// but the capsule was created before `extraction` had a sensible
		// default. Tell them exactly what to do rather than silently
		// showing "0 records".
		const hint = this.contentEl.createEl("p");
		hint.appendText("This capsule has ");
		hint.createEl("strong", {
			text: `${sourceCount} source${sourceCount === 1 ? "" : "s"}`,
		});
		hint.appendText(" but ");
		hint.createEl("code", { text: "extraction: none" });
		hint.appendText(
			" — no records will be produced. Set the ",
		);
		hint.createEl("code", { text: "extraction" });
		hint.appendText(" property to one of ");
		hint.createEl("code", { text: "frontmatter-list" });
		hint.appendText(", ");
		hint.createEl("code", { text: "table" });
		hint.appendText(", or ");
		hint.createEl("code", { text: "code-fence" });
		hint.appendText(' and re-run "Preview capsule data".');
	}

	private renderSummary(
		containerEl: HTMLElement,
		result: ExtractionResult,
		sourceCount: number,
	): void {
		const summary = containerEl.createEl("p");
		summary.createEl("strong", { text: `${result.records.length} record(s) ` });
		summary.appendText(
			`from ${sourceCount} source${sourceCount === 1 ? "" : "s"}`,
		);
		if (result.errors.length > 0) {
			summary.appendText(` · ${result.errors.length} error(s)`);
		}
	}

	private renderErrors(
		containerEl: HTMLElement,
		errors: ExtractionError[],
	): void {
		containerEl.createEl("h3", { text: "Errors" });
		const ul = containerEl.createEl("ul");
		for (const err of errors) {
			const li = ul.createEl("li");
			li.createEl("code", { text: err.source });
			li.appendText(` — ${err.message}`);
		}
	}

	private renderRecords(
		containerEl: HTMLElement,
		records: Record<string, unknown>[],
	): void {
		containerEl.createEl("h3", {
			text:
				records.length > MAX_RECORDS_SHOWN
					? `Records (first ${MAX_RECORDS_SHOWN} of ${records.length})`
					: `Records (${records.length})`,
		});

		// Column set is the union of keys in the shown records, in first-
		// seen order (stable across runs since JS object iteration order
		// is insertion order).
		const columns: string[] = [];
		const seen = new Set<string>();
		const shown = records.slice(0, MAX_RECORDS_SHOWN);
		for (const rec of shown) {
			for (const k of Object.keys(rec)) {
				if (!seen.has(k)) {
					seen.add(k);
					columns.push(k);
				}
			}
		}

		const tableWrapper = containerEl.createEl("div", {
			attr: { style: "overflow-x: auto; max-height: 40vh;" },
		});
		const table = tableWrapper.createEl("table", {
			attr: { style: "border-collapse: collapse; font-size: 0.85em;" },
		});
		const thead = table.createEl("thead");
		const headerRow = thead.createEl("tr");
		for (const col of columns) {
			headerRow.createEl("th", {
				text: col,
				attr: {
					style:
						"text-align: left; padding: 0.25em 0.5em; border-bottom: 1px solid var(--background-modifier-border);",
				},
			});
		}
		const tbody = table.createEl("tbody");
		for (const rec of shown) {
			const tr = tbody.createEl("tr");
			for (const col of columns) {
				tr.createEl("td", {
					text: formatCell(rec[col]),
					attr: {
						style:
							"padding: 0.2em 0.5em; border-bottom: 1px solid var(--background-modifier-border); vertical-align: top;",
					},
				});
			}
		}
	}

	private addCloseButton(): void {
		new Setting(this.contentEl).addButton((btn) =>
			btn
				.setButtonText("Close")
				.setCta()
				.onClick(() => this.close()),
		);
	}
}

function formatCell(value: unknown): string {
	if (value == null) return "";
	if (typeof value === "string") return value;
	if (typeof value === "number" || typeof value === "boolean") return String(value);
	if (Array.isArray(value)) return value.map((v) => formatCell(v)).join(", ");
	try {
		return JSON.stringify(value);
	} catch {
		return String(value);
	}
}
