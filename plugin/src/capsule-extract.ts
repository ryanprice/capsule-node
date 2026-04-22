import { App, TFile } from "obsidian";
import {
	extract,
	ExtractionResult,
	needsContent,
	parseSourceRef,
	ResolvedSource,
} from "./extraction";
import { Manifest } from "./manifest";

/**
 * Resolve a capsule's sources against Obsidian's metadata cache + vault,
 * then run the configured extraction mode. Shared between the preview
 * modal (read-only) and the "Publish capsule" command (which hands the
 * records to the daemon for encryption).
 *
 * For frontmatter-list we skip vault.read — the metadata cache already
 * has everything we need. For table and code-fence modes we read the
 * full body once per source, in parallel.
 */
export async function runExtraction(
	app: App,
	capsuleFile: TFile,
	manifest: Manifest,
): Promise<ExtractionResult> {
	const sources = manifest.sources ?? [];
	const mode = manifest.extraction ?? "none";
	const withContent = needsContent(mode);
	const resolved = await Promise.all(
		sources.map(async (raw): Promise<ResolvedSource> => {
			const linkpath = parseSourceRef(raw);
			const file = app.metadataCache.getFirstLinkpathDest(
				linkpath,
				capsuleFile.path,
			);
			if (!file) {
				return {
					rawRef: raw,
					path: linkpath,
					frontmatter: null,
					content: null,
				};
			}
			const cache = app.metadataCache.getFileCache(file);
			const frontmatter =
				(cache?.frontmatter ?? null) as Record<string, unknown> | null;
			const content = withContent ? await app.vault.read(file) : null;
			return { rawRef: raw, path: file.path, frontmatter, content };
		}),
	);
	return extract(resolved, mode);
}
