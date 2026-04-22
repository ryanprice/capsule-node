import { App, Modal, Setting, TFile } from "obsidian";
import {
	ExtractionError,
	ExtractionResult,
	extract,
	parseSourceRef,
	ResolvedSource,
} from "./extraction";
import { Manifest } from "./manifest";

const MAX_RECORDS_SHOWN = 20;

/**
 * Preview-only view into what a capsule would expose.
 *
 * Never writes to disk, never talks to the daemon, never encrypts. The
 * whole point is to answer "what am I actually about to share?" before a
 * user flips a capsule to `active`. Everything is read through Obsidian's
 * metadata cache; no vault.read() required for frontmatter-list mode.
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

		const resolved = sources.map((raw) => this.resolveSource(raw));
		const result = extract(resolved, this.manifest.extraction ?? "none");

		this.renderSummary(contentEl, result, sources.length);
		if (result.errors.length > 0) {
			this.renderErrors(contentEl, result.errors);
		}
		if (result.records.length > 0) {
			this.renderRecords(contentEl, result.records);
		}
		this.addCloseButton();
	}

	onClose(): void {
		this.contentEl.empty();
	}

	private resolveSource(raw: string): ResolvedSource {
		const linkpath = parseSourceRef(raw);
		const file = this.app.metadataCache.getFirstLinkpathDest(
			linkpath,
			this.capsuleFile.path,
		);
		if (!file) {
			return { rawRef: raw, path: linkpath, frontmatter: null };
		}
		const cache = this.app.metadataCache.getFileCache(file);
		return {
			rawRef: raw,
			path: file.path,
			frontmatter: (cache?.frontmatter ?? null) as Record<string, unknown> | null,
		};
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

		// Column set is the union of keys in the first N records, stable
		// across page loads (insertion order preserved).
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
