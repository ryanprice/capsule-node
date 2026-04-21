import {
	App,
	MarkdownPostProcessorContext,
	Plugin,
	TFile,
} from "obsidian";
import { CapsuleStatus } from "./manifest";
import { isCapsuleNotePath, statusBadge, statusFromFrontmatter } from "./view";

const BANNER_CLASS = "capsule-status-banner";

/**
 * Renders status badges for capsule notes in two places:
 *   1. The status bar (always visible, reflects the currently-active file).
 *   2. Inline at the top of each capsule note in reading view.
 *
 * Both surfaces read status from Obsidian's metadata cache, so they update
 * automatically within ~100ms of a frontmatter edit being saved.
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

	/**
	 * Wire up event listeners via the plugin so they're automatically torn
	 * down on unload. Also register the reading-view post-processor.
	 */
	register(): void {
		this.plugin.registerEvent(
			this.app.workspace.on("active-leaf-change", () => {
				this.refreshStatusBar();
			}),
		);
		this.plugin.registerEvent(
			this.app.metadataCache.on("changed", (file: TFile) => {
				// Only care about the file that's currently in the foreground —
				// a background capsule changing doesn't affect the status bar.
				const activeFile = this.app.workspace.getActiveFile();
				if (activeFile && activeFile.path === file.path) {
					this.updateStatusBarForFile(activeFile);
				}
			}),
		);
		this.plugin.registerMarkdownPostProcessor((el, ctx) => {
			this.decorateReadingView(el, ctx);
		});

		// Seed: if a capsule note is already active when the plugin loads.
		this.refreshStatusBar();
	}

	private refreshStatusBar(): void {
		this.updateStatusBarForFile(this.app.workspace.getActiveFile());
	}

	private updateStatusBarForFile(file: TFile | null): void {
		if (!file || !isCapsuleNotePath(file.path, this.capsuleFolder())) {
			this.hideStatusBar();
			return;
		}
		const fm = this.app.metadataCache.getFileCache(file)?.frontmatter;
		const status = statusFromFrontmatter(
			fm as Record<string, unknown> | undefined,
		);
		if (!status) {
			this.hideStatusBar();
			return;
		}
		this.showStatusBar(status);
	}

	private showStatusBar(status: CapsuleStatus): void {
		const badge = statusBadge(status);
		this.statusBarEl.empty();
		this.statusBarEl.removeClass(
			"capsule-status-active",
			"capsule-status-paused",
			"capsule-status-draft",
			"capsule-status-archived",
		);
		this.statusBarEl.addClass(badge.cssClass);
		this.statusBarEl.setText(`${badge.glyph} capsule · ${badge.label}`);
		this.statusBarEl.style.display = "";
	}

	private hideStatusBar(): void {
		this.statusBarEl.style.display = "none";
	}

	private decorateReadingView(
		el: HTMLElement,
		ctx: MarkdownPostProcessorContext,
	): void {
		const isCapsule = isCapsuleNotePath(ctx.sourcePath, this.capsuleFolder());
		console.log("[capsule] post-processor fired", {
			sourcePath: ctx.sourcePath,
			capsuleFolder: this.capsuleFolder(),
			isCapsule,
			hasFrontmatter: !!ctx.frontmatter,
			status: (ctx.frontmatter as Record<string, unknown> | undefined)?.status,
		});
		if (!isCapsule) return;

		requestAnimationFrame(() => {
			const sizer = el.closest<HTMLElement>(".markdown-preview-sizer");
			console.log("[capsule] rAF fired", {
				elAttached: el.isConnected,
				sizerViaClosest: !!sizer,
				elParent: el.parentElement?.className,
			});
			if (!sizer) return;

			const status = statusFromFrontmatter(
				ctx.frontmatter as Record<string, unknown> | null | undefined,
			);
			console.log("[capsule] status resolved", status);

			const existing = sizer.querySelector(`:scope > .${BANNER_CLASS}`);
			if (!status) {
				existing?.remove();
				return;
			}
			const banner = buildBanner(status);
			if (existing) {
				existing.replaceWith(banner);
			} else {
				sizer.prepend(banner);
			}
		});
	}
}

function buildBanner(status: CapsuleStatus): HTMLElement {
	const badge = statusBadge(status);
	const el = document.createElement("div");
	el.addClass(BANNER_CLASS, badge.cssClass);
	el.setText(`${badge.glyph} capsule · ${badge.label}`);
	return el;
}
