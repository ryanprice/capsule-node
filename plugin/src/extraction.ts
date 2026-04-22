import { ExtractionMode } from "./manifest";

/**
 * Output of an extraction pass over a capsule's source notes.
 * `records` is what would be exposed to an agent; `errors` is what the
 * plugin couldn't resolve or parse. Both are surfaced to the user in the
 * preview modal so nothing goes live without being seen.
 */
export interface ExtractionResult {
	records: Record<string, unknown>[];
	errors: ExtractionError[];
}

export interface ExtractionError {
	source: string;
	message: string;
}

/**
 * Strip a wikilink's `[[…]]` wrapper and `|alias` suffix, leaving just
 * the link path. Accepts plain paths unchanged.
 */
export function parseSourceRef(raw: string): string {
	let s = raw.trim();
	if (s.startsWith("[[") && s.endsWith("]]")) {
		s = s.slice(2, -2);
	}
	const pipe = s.indexOf("|");
	if (pipe !== -1) {
		s = s.slice(0, pipe);
	}
	return s.trim();
}

/**
 * A resolved source ready for extraction. `path` is the vault-relative
 * path to the note (for error messages); `frontmatter` is whatever the
 * caller read from Obsidian's metadata cache for that note.
 */
export interface ResolvedSource {
	rawRef: string;
	path: string;
	frontmatter: Record<string, unknown> | null;
}

/**
 * Pure extractor: given already-resolved sources, produce records +
 * errors. Kept dependency-free so tests can construct ResolvedSource
 * objects by hand without an Obsidian runtime.
 */
export function extract(
	sources: ResolvedSource[],
	mode: ExtractionMode,
): ExtractionResult {
	if (mode === "none") {
		return { records: [], errors: [] };
	}
	if (mode === "frontmatter-list") {
		return extractFrontmatterList(sources);
	}
	// Exhaustiveness: ExtractionMode is a closed union; if a new variant
	// is added without a branch here, TS will catch it at compile time.
	const exhaustive: never = mode;
	throw new Error(`unsupported extraction mode: ${exhaustive as string}`);
}

function extractFrontmatterList(sources: ResolvedSource[]): ExtractionResult {
	const records: Record<string, unknown>[] = [];
	const errors: ExtractionError[] = [];
	for (const src of sources) {
		if (!src.frontmatter) {
			errors.push({
				source: src.rawRef,
				message: `source has no frontmatter (expected a YAML block)`,
			});
			continue;
		}
		// `position` is Obsidian's own metadata-cache bookkeeping; it's
		// never part of user-authored frontmatter and must not leak into
		// what agents see.
		const { position: _pos, ...userData } = src.frontmatter as Record<
			string,
			unknown
		> & { position?: unknown };
		void _pos;
		records.push(userData);
	}
	return { records, errors };
}
