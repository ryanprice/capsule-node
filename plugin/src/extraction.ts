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
 * A resolved source ready for extraction. `frontmatter` comes from
 * Obsidian's metadata cache; `content` is the full note body (including
 * frontmatter text) — only populated for modes that need it (table,
 * code-fence). `null` means the caller didn't read the file.
 */
export interface ResolvedSource {
	rawRef: string;
	path: string;
	frontmatter: Record<string, unknown> | null;
	content: string | null;
}

/** True iff the mode requires the caller to read the note's full content
 * (vault.read). frontmatter-list is satisfied by the metadata cache alone. */
export function needsContent(mode: ExtractionMode): boolean {
	return mode === "table" || mode === "code-fence";
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
	switch (mode) {
		case "none":
			return { records: [], errors: [] };
		case "frontmatter-list":
			return extractFrontmatterList(sources);
		case "table":
			return extractTable(sources);
		case "code-fence":
			return extractCodeFence(sources);
		default: {
			const exhaustive: never = mode;
			throw new Error(`unsupported extraction mode: ${exhaustive as string}`);
		}
	}
}

// ─── frontmatter-list ───────────────────────────────────────────────

function extractFrontmatterList(sources: ResolvedSource[]): ExtractionResult {
	const records: Record<string, unknown>[] = [];
	const errors: ExtractionError[] = [];
	for (const src of sources) {
		if (!src.frontmatter) {
			errors.push({
				source: src.rawRef,
				message: "source has no frontmatter (expected a YAML block)",
			});
			continue;
		}
		records.push(stripPosition(src.frontmatter));
	}
	return { records, errors };
}

function stripPosition(
	fm: Record<string, unknown>,
): Record<string, unknown> {
	// `position` is Obsidian's own metadata-cache bookkeeping; it's never
	// part of user-authored frontmatter and must not leak into what agents
	// see.
	const { position: _pos, ...userData } = fm as Record<string, unknown> & {
		position?: unknown;
	};
	void _pos;
	return userData;
}

// ─── table ──────────────────────────────────────────────────────────

function extractTable(sources: ResolvedSource[]): ExtractionResult {
	const records: Record<string, unknown>[] = [];
	const errors: ExtractionError[] = [];
	for (const src of sources) {
		if (src.content == null) {
			errors.push({
				source: src.rawRef,
				message: "source content not read — internal error",
			});
			continue;
		}
		const table = parseMarkdownTable(src.content);
		if (!table) {
			errors.push({
				source: src.rawRef,
				message: "no markdown table found in this note",
			});
			continue;
		}
		for (const row of table.rows) {
			const rec: Record<string, unknown> = {};
			for (let i = 0; i < table.headers.length; i++) {
				rec[table.headers[i]] = row[i] ?? "";
			}
			records.push(rec);
		}
	}
	return { records, errors };
}

interface ParsedTable {
	headers: string[];
	rows: string[][];
}

/**
 * Finds the first markdown table in `content` and returns its headers +
 * data rows. Skips YAML frontmatter. Handles optional leading/trailing
 * pipes and alignment colons in the separator. Returns `null` if no
 * table exists in the note.
 */
export function parseMarkdownTable(content: string): ParsedTable | null {
	const lines = stripFrontmatterLines(content).split("\n");
	for (let i = 0; i < lines.length - 1; i++) {
		const headerLine = lines[i];
		const sepLine = lines[i + 1];
		if (!looksLikeTableRow(headerLine)) continue;
		if (!isTableSeparator(sepLine)) continue;
		const headers = splitTableRow(headerLine);
		if (headers.length === 0) continue;
		const rows: string[][] = [];
		let j = i + 2;
		while (j < lines.length && looksLikeTableRow(lines[j])) {
			rows.push(splitTableRow(lines[j]));
			j++;
		}
		return { headers, rows };
	}
	return null;
}

function looksLikeTableRow(line: string): boolean {
	const t = line.trim();
	return t.includes("|") && t !== "";
}

function isTableSeparator(line: string): boolean {
	const t = line.trim();
	if (!t.includes("-")) return false;
	const cells = splitTableRow(line);
	if (cells.length === 0) return false;
	return cells.every((c) => /^:?-+:?$/.test(c.trim()));
}

function splitTableRow(line: string): string[] {
	const trimmed = line.trim().replace(/^\||\|$/g, "");
	return trimmed.split("|").map((c) => c.trim());
}

function stripFrontmatterLines(content: string): string {
	if (!content.startsWith("---\n")) return content;
	const close = content.indexOf("\n---", 4);
	if (close === -1) return content;
	const afterClose = content.indexOf("\n", close + 1);
	return afterClose === -1 ? "" : content.slice(afterClose + 1);
}

// ─── code-fence ─────────────────────────────────────────────────────

function extractCodeFence(sources: ResolvedSource[]): ExtractionResult {
	const records: Record<string, unknown>[] = [];
	const errors: ExtractionError[] = [];
	for (const src of sources) {
		if (src.content == null) {
			errors.push({
				source: src.rawRef,
				message: "source content not read — internal error",
			});
			continue;
		}
		const fences = findCapsuleDataFences(src.content);
		if (fences.length === 0) {
			errors.push({
				source: src.rawRef,
				message: "no ```capsule-data``` fence found in this note",
			});
			continue;
		}
		for (const fence of fences) {
			try {
				const parsed = parseFenceBody(fence.body, fence.lang);
				records.push(...parsed);
			} catch (err) {
				const message = err instanceof Error ? err.message : String(err);
				errors.push({
					source: src.rawRef,
					message: `capsule-data[${fence.lang}] fence failed to parse: ${message}`,
				});
			}
		}
	}
	return { records, errors };
}

export interface CapsuleDataFence {
	lang: string;
	body: string;
}

export function findCapsuleDataFences(content: string): CapsuleDataFence[] {
	const out: CapsuleDataFence[] = [];
	const lines = content.split("\n");
	let i = 0;
	while (i < lines.length) {
		const match = /^```capsule-data(?::([a-zA-Z0-9_-]+))?\s*$/.exec(lines[i]);
		if (!match) {
			i++;
			continue;
		}
		const lang = (match[1] ?? "json").toLowerCase();
		const start = i + 1;
		let end = start;
		while (end < lines.length && !/^```\s*$/.test(lines[end])) {
			end++;
		}
		out.push({ lang, body: lines.slice(start, end).join("\n") });
		i = end + 1;
	}
	return out;
}

function parseFenceBody(
	body: string,
	lang: string,
): Record<string, unknown>[] {
	if (lang === "json") return parseJsonArray(body);
	if (lang === "csv") return parseCsv(body);
	throw new Error(
		`unsupported language '${lang}' (supported: json, csv)`,
	);
}

function parseJsonArray(body: string): Record<string, unknown>[] {
	const parsed = JSON.parse(body);
	if (!Array.isArray(parsed)) {
		throw new Error("JSON body must be an array");
	}
	return parsed.map((item, i) => {
		if (item === null || typeof item !== "object" || Array.isArray(item)) {
			throw new Error(`element ${i} is not an object`);
		}
		return item as Record<string, unknown>;
	});
}

function parseCsv(body: string): Record<string, unknown>[] {
	const lines = body.split("\n").filter((l) => l.trim() !== "");
	if (lines.length === 0) return [];
	const headers = splitCsvRow(lines[0]);
	return lines.slice(1).map((line) => {
		const cells = splitCsvRow(line);
		const rec: Record<string, unknown> = {};
		for (let i = 0; i < headers.length; i++) {
			rec[headers[i]] = cells[i] ?? "";
		}
		return rec;
	});
}

function splitCsvRow(line: string): string[] {
	// Minimal CSV: comma-separated, no quoted-comma handling. Good enough
	// for user-pasted CGM-style data; upgrade to a real parser if users
	// need quoting.
	return line.split(",").map((c) => c.trim());
}
