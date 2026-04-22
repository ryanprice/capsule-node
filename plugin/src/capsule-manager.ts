import { App, normalizePath, TFile } from "obsidian";
import { buildCapsuleNote, parseCapsuleNote } from "./frontmatter";
import { generateCapsuleId, isValidCapsuleId, Manifest } from "./manifest";

/**
 * Writes capsule notes (the human-facing source of truth) and derives the
 * daemon-facing JSON manifest from them. The daemon watches
 * `.capsule/manifests/`; the plugin is the only writer of that directory.
 */
export class CapsuleManager {
	constructor(
		private app: App,
		private capsuleFolder: () => string,
	) {}

	/**
	 * Create a new draft capsule note and write its manifest. Returns the
	 * note's TFile so callers can open it.
	 */
	async createDraftCapsule(
		params: { schema?: string; floorPrice?: string } = {}
	): Promise<{ file: TFile; manifest: Manifest }> {
		const manifest: Manifest = {
			capsule_id: generateCapsuleId(),
			schema: params.schema ?? "capsule://draft",
			status: "draft",
			floor_price: params.floorPrice ?? "0.01 USDC/query",
			computation_classes: ["A"],
			tags: [],
			sources: [],
			// Default to the real extractor — a fresh capsule with `none`
			// is useless until the user thinks to flip it. With
			// frontmatter-list as the default, adding a wikilink to
			// sources immediately produces records in the preview.
			extraction: "frontmatter-list",
		};

		const folder = this.capsuleFolder();
		await this.ensureDir(folder);
		const notePath = normalizePath(`${folder}/${manifest.capsule_id}.md`);
		const body = buildCapsuleNote({ manifest });
		const file = await this.app.vault.create(notePath, body);

		await this.writeManifestJson(manifest);
		return { file, manifest };
	}

	/**
	 * Sync a capsule note → manifest JSON. Called from the vault modify
	 * listener whenever a capsule note changes. Errors are thrown; the
	 * caller decides how to surface them (debounced logger, not per-keystroke
	 * notices).
	 */
	async syncNoteToManifest(file: TFile): Promise<Manifest> {
		const content = await this.app.vault.read(file);
		const parsed = parseCapsuleNote(content);
		await this.writeManifestJson(parsed.manifest);
		return parsed.manifest;
	}

	/** Check whether a path lives under the configured capsule folder. */
	isCapsuleNotePath(path: string): boolean {
		const folder = this.capsuleFolder();
		const prefix = folder.endsWith("/") ? folder : `${folder}/`;
		return path.startsWith(prefix) && path.endsWith(".md");
	}

	/**
	 * Write the manifest atomically: emit to `<cid>.json.tmp`, then rename.
	 * The daemon's watcher filters out `.tmp` paths, so it only ever sees a
	 * complete file at the final name. Combined with the watcher's per-path
	 * debounce, this eliminates the empty-file-mid-write warnings observed
	 * in production (Obsidian adapter writes → Syncthing propagation → fs
	 * watcher burst).
	 */
	private async writeManifestJson(manifest: Manifest): Promise<void> {
		if (!isValidCapsuleId(manifest.capsule_id)) {
			throw new Error(`invalid capsule_id: ${manifest.capsule_id}`);
		}
		const dir = normalizePath(".capsule/manifests");
		await this.ensureDir(dir);
		const finalPath = normalizePath(`${dir}/${manifest.capsule_id}.json`);
		const tmpPath = `${finalPath}.tmp`;
		const body = JSON.stringify(manifest, null, 2) + "\n";
		const adapter = this.app.vault.adapter;

		// Clean up any stale tempfile from a crashed previous write.
		if (await adapter.exists(tmpPath)) {
			await adapter.remove(tmpPath);
		}
		await adapter.write(tmpPath, body);

		// Obsidian's adapter.rename does not guarantee "replace existing"
		// across platforms; remove first, then rename. The watcher's debounce
		// coalesces the remove + rename into a single registry update.
		if (await adapter.exists(finalPath)) {
			await adapter.remove(finalPath);
		}
		await adapter.rename(tmpPath, finalPath);
	}

	private async ensureDir(path: string): Promise<void> {
		const adapter = this.app.vault.adapter;
		if (await adapter.exists(path)) return;
		const segments = normalizePath(path).split("/");
		for (let i = 1; i <= segments.length; i++) {
			const partial = segments.slice(0, i).join("/");
			if (partial && !(await adapter.exists(partial))) {
				await adapter.mkdir(partial);
			}
		}
	}
}
