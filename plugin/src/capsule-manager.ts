import { App, normalizePath } from "obsidian";
import { generateCapsuleId, isValidCapsuleId, Manifest } from "./manifest";

/**
 * Writes and lists capsule manifests inside the Obsidian vault's `.capsule/`
 * directory. The daemon watches this directory and picks up new files;
 * the plugin is the only writer.
 */
export class CapsuleManager {
	constructor(private app: App) {}

	/** Create a minimal "draft" capsule and write its manifest to disk. */
	async createDraftCapsule(params: {
		schema?: string;
		floorPrice?: string;
	} = {}): Promise<Manifest> {
		const manifest: Manifest = {
			capsule_id: generateCapsuleId(),
			schema: params.schema ?? "capsule://draft",
			status: "draft",
			floor_price: params.floorPrice ?? "0.01 USDC/query",
			computation_classes: ["A"],
			tags: [],
		};
		await this.writeManifest(manifest);
		return manifest;
	}

	async writeManifest(manifest: Manifest): Promise<void> {
		if (!isValidCapsuleId(manifest.capsule_id)) {
			throw new Error(`invalid capsule_id: ${manifest.capsule_id}`);
		}
		const dir = normalizePath(".capsule/manifests");
		await this.ensureDir(dir);
		const path = normalizePath(`${dir}/${manifest.capsule_id}.json`);
		const body = JSON.stringify(manifest, null, 2) + "\n";
		const adapter = this.app.vault.adapter;
		if (await adapter.exists(path)) {
			await adapter.write(path, body);
		} else {
			await adapter.write(path, body);
		}
	}

	async listManifestFiles(): Promise<string[]> {
		const dir = normalizePath(".capsule/manifests");
		const adapter = this.app.vault.adapter;
		if (!(await adapter.exists(dir))) return [];
		const entries = await adapter.list(dir);
		return entries.files.filter((f) => f.endsWith(".json"));
	}

	private async ensureDir(path: string): Promise<void> {
		const adapter = this.app.vault.adapter;
		if (await adapter.exists(path)) return;
		// Obsidian's adapter.mkdir is recursive-equivalent; create each segment
		// so it works even if `.capsule/` itself doesn't exist yet.
		const segments = path.split("/");
		for (let i = 1; i <= segments.length; i++) {
			const partial = segments.slice(0, i).join("/");
			if (!(await adapter.exists(partial))) {
				await adapter.mkdir(partial);
			}
		}
	}
}
