import { App, MarkdownView, Plugin, TFile } from "obsidian";
import { CapsuleStatus } from "./manifest";
import { isCapsuleNotePath, statusBadge, statusFromFrontmatter } from "./view";

const STATUS_CLASSES = [
	"capsule-status-active",
	"capsule-status-paused",
	"capsule-status-draft",
	"capsule-status-archived",
] as const;

/**
 * Renders status badges for capsule notes in two places:
 *   1. The status bar (always visible, reflects the currently-active file).
 *   2. A pill rendered via CSS ::before on the reading-view container for
 *      capsule notes.
 *
 * The reading-view badge deliberately avoids injecting DOM children into
 * Obsidian's render tree — Obsidian tears down and rebuilds the markdown
 * sizer's children on every section re-render, which would wipe any
 * prepended element. A pseudo-element driven by a class on a container
 * Obsidian treats as stable survives all of that.
 */
export class CapsuleDecorator {
	private statusBarEl: HTMLElement;

	constructor(
		private plugin: Plugin,
		private app: App,
		private capsuleFolder: () => string,
	) {
		this.statusBarEl = plugin.addStatusBarItem();
		this.statusBarEl.addClass("capsule-status-bar");
		this.hideStatusBar();
	}

	register(): void {
		this.plugin.registerEvent(
			this.app.workspace.on("active-leaf-change", () => {
				this.refresh();
			}),
		);
		this.plugin.registerEvent(
			this.app.workspace.on("layout-change", () => {
				this.refresh();
			}),
		);
		this.plugin.registerEvent(
			this.app.metadataCache.on("changed", (file: TFile) => {
				const activeFile = this.app.workspace.getActiveFile();
				if (activeFile && activeFile.path === file.path) {
					this.refresh();
				}
			}),
		);
		this.refresh();
	}

	private refresh(): void {
		const activeFile = this.app.workspace.getActiveFile();
		const status = this.statusFor(activeFile);
		this.updateStatusBar(status);
		this.updateAllLeafContainers(activeFile, status);
	}

	private statusFor(file: TFile | null): CapsuleStatus | null {
		if (!file || !isCapsuleNotePath(file.path, this.capsuleFolder())) {
			return null;
		}
		const fm = this.app.metadataCache.getFileCache(file)?.frontmatter;
		return statusFromFrontmatter(fm as Record<string, unknown> | undefined);
	}

	private updateStatusBar(status: CapsuleStatus | null): void {
		if (!status) {
			this.hideStatusBar();
			return;
		}
		const badge = statusBadge(status);
		this.statusBarEl.empty();
		this.statusBarEl.removeClass(...STATUS_CLASSES);
		this.statusBarEl.addClass(badge.cssClass);
		this.statusBarEl.setText(`${badge.glyph} capsule · ${badge.label}`);
		this.statusBarEl.style.display = "";
	}

	private hideStatusBar(): void {
		this.statusBarEl.style.display = "none";
	}

	/**
	 * Apply or clear the `capsule-status-<status>` class on each markdown
	 * leaf's content container. The class is placed on `view.contentEl` so
	 * it's above both editor and reading modes — the CSS `::before` rule
	 * only renders inside the reading view.
	 *
	 * A leaf that doesn't correspond to the currently-active capsule file
	 * (or isn't a capsule at all) gets its classes stripped, so stale badges
	 * from a previous view don't linger.
	 */
	private updateAllLeafContainers(
		activeFile: TFile | null,
		activeStatus: CapsuleStatus | null,
	): void {
		this.app.workspace.iterateAllLeaves((leaf) => {
			const view = leaf.view;
			if (!(view instanceof MarkdownView)) return;
			const container = view.contentEl;
			container.removeClass(...STATUS_CLASSES);
			if (
				activeStatus &&
				activeFile &&
				view.file?.path === activeFile.path
			) {
				container.addClass(statusBadge(activeStatus).cssClass);
			}
		});
	}
}
