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

	private async writeManifestJson(manifest: Manifest): Promise<void> {
		if (!isValidCapsuleId(manifest.capsule_id)) {
			throw new Error(`invalid capsule_id: ${manifest.capsule_id}`);
		}
		const dir = normalizePath(".capsule/manifests");
		await this.ensureDir(dir);
		const path = normalizePath(`${dir}/${manifest.capsule_id}.json`);
		const body = JSON.stringify(manifest, null, 2) + "\n";
		await this.app.vault.adapter.write(path, body);
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
