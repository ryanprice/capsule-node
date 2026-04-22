import { strict as assert } from "node:assert";
import { describe, it } from "node:test";

import {
	extract,
	findCapsuleDataFences,
	needsContent,
	parseMarkdownTable,
	parseSourceRef,
	ResolvedSource,
} from "./extraction";

function resolved(
	rawRef: string,
	path: string,
	frontmatter: Record<string, unknown> | null,
	content: string | null = null,
): ResolvedSource {
	return { rawRef, path, frontmatter, content };
}

describe("parseSourceRef", () => {
	it("strips wikilink brackets", () => {
		assert.equal(parseSourceRef("[[foo]]"), "foo");
		assert.equal(parseSourceRef("[[raw/cgm-2026-01]]"), "raw/cgm-2026-01");
	});
	it("strips alias suffix", () => {
		assert.equal(parseSourceRef("[[foo|Friendly Name]]"), "foo");
		assert.equal(parseSourceRef("foo|alias"), "foo");
	});
	it("passes plain paths through unchanged", () => {
		assert.equal(parseSourceRef("raw/cgm-2026-01"), "raw/cgm-2026-01");
	});
	it("trims whitespace", () => {
		assert.equal(parseSourceRef("  [[foo]]  "), "foo");
	});
});

describe("needsContent", () => {
	it("returns true for modes that need vault.read", () => {
		assert.equal(needsContent("table"), true);
		assert.equal(needsContent("code-fence"), true);
	});
	it("returns false for frontmatter-list and none", () => {
		assert.equal(needsContent("frontmatter-list"), false);
		assert.equal(needsContent("none"), false);
	});
});

describe("extract (frontmatter-list)", () => {
	it("returns empty result for empty sources", () => {
		const result = extract([], "frontmatter-list");
		assert.deepEqual(result, { records: [], errors: [] });
	});

	it("extracts one record per source with frontmatter", () => {
		const sources = [
			resolved("[[r1]]", "raw/r1.md", { glucose: 95, tag: "morning" }),
			resolved("[[r2]]", "raw/r2.md", { glucose: 110, tag: "afternoon" }),
		];
		const result = extract(sources, "frontmatter-list");
		assert.equal(result.records.length, 2);
		assert.deepEqual(result.records[0], { glucose: 95, tag: "morning" });
		assert.deepEqual(result.records[1], { glucose: 110, tag: "afternoon" });
		assert.equal(result.errors.length, 0);
	});

	it("records an error for sources with no frontmatter", () => {
		const sources = [
			resolved("[[good]]", "good.md", { x: 1 }),
			resolved("[[bad]]", "bad.md", null),
		];
		const result = extract(sources, "frontmatter-list");
		assert.equal(result.records.length, 1);
		assert.equal(result.errors.length, 1);
		assert.equal(result.errors[0].source, "[[bad]]");
		assert.match(result.errors[0].message, /no frontmatter/);
	});

	it("strips Obsidian's internal `position` field", () => {
		const sources = [
			resolved("[[r]]", "r.md", {
				glucose: 95,
				position: { start: 0, end: 100 },
			}),
		];
		const result = extract(sources, "frontmatter-list");
		assert.deepEqual(result.records[0], { glucose: 95 });
	});

	it("preserves array-valued frontmatter fields", () => {
		const sources = [
			resolved("[[r]]", "r.md", { tags: ["cgm", "2026"], value: 95 }),
		];
		const result = extract(sources, "frontmatter-list");
		assert.deepEqual(result.records[0].tags, ["cgm", "2026"]);
		assert.equal(result.records[0].value, 95);
	});
});

describe("extract (none)", () => {
	it("always returns empty", () => {
		const sources = [resolved("[[r]]", "r.md", { x: 1 })];
		const result = extract(sources, "none");
		assert.deepEqual(result, { records: [], errors: [] });
	});
});

describe("parseMarkdownTable", () => {
	it("parses a standard pipe table", () => {
		const md = [
			"# Glucose readings",
			"",
			"| timestamp | mg_dl |",
			"|-----------|-------|",
			"| 06:00     | 95    |",
			"| 06:05     | 98    |",
			"",
			"More prose below.",
		].join("\n");
		const table = parseMarkdownTable(md);
		assert.ok(table);
		assert.deepEqual(table!.headers, ["timestamp", "mg_dl"]);
		assert.deepEqual(table!.rows, [
			["06:00", "95"],
			["06:05", "98"],
		]);
	});

	it("tolerates alignment colons in the separator", () => {
		const md = [
			"| a | b |",
			"|:--|--:|",
			"| 1 | 2 |",
		].join("\n");
		const table = parseMarkdownTable(md);
		assert.ok(table);
		assert.deepEqual(table!.headers, ["a", "b"]);
		assert.deepEqual(table!.rows, [["1", "2"]]);
	});

	it("skips YAML frontmatter when searching", () => {
		const md = [
			"---",
			"type: note",
			"---",
			"",
			"| a | b |",
			"|---|---|",
			"| 1 | 2 |",
		].join("\n");
		const table = parseMarkdownTable(md);
		assert.ok(table);
		assert.deepEqual(table!.headers, ["a", "b"]);
	});

	it("returns null when no table is present", () => {
		const md = "# A heading\n\nJust prose. | Not a table |.\n";
		assert.equal(parseMarkdownTable(md), null);
	});
});

describe("extract (table)", () => {
	it("produces one record per data row, columns from headers", () => {
		const content = [
			"| timestamp | mg_dl |",
			"|-----------|-------|",
			"| 06:00     | 95    |",
			"| 06:05     | 98    |",
		].join("\n");
		const sources = [resolved("[[r]]", "r.md", null, content)];
		const result = extract(sources, "table");
		assert.equal(result.records.length, 2);
		assert.deepEqual(result.records[0], { timestamp: "06:00", mg_dl: "95" });
		assert.deepEqual(result.records[1], { timestamp: "06:05", mg_dl: "98" });
		assert.equal(result.errors.length, 0);
	});

	it("errors per-source when no table exists", () => {
		const sources = [
			resolved("[[r]]", "r.md", null, "# No table here\n\nJust text."),
		];
		const result = extract(sources, "table");
		assert.equal(result.records.length, 0);
		assert.equal(result.errors.length, 1);
		assert.match(result.errors[0].message, /no markdown table/);
	});
});

describe("findCapsuleDataFences", () => {
	it("finds a single fence with default json lang", () => {
		const content = [
			"Some prose.",
			"",
			"```capsule-data",
			'[{"x":1}]',
			"```",
			"",
			"More prose.",
		].join("\n");
		const fences = findCapsuleDataFences(content);
		assert.equal(fences.length, 1);
		assert.equal(fences[0].lang, "json");
		assert.equal(fences[0].body, '[{"x":1}]');
	});

	it("respects an explicit lang hint", () => {
		const content = [
			"```capsule-data:csv",
			"a,b",
			"1,2",
			"```",
		].join("\n");
		const fences = findCapsuleDataFences(content);
		assert.equal(fences.length, 1);
		assert.equal(fences[0].lang, "csv");
	});

	it("finds multiple fences", () => {
		const content = [
			"```capsule-data",
			'[{"x":1}]',
			"```",
			"",
			"```capsule-data",
			'[{"x":2}]',
			"```",
		].join("\n");
		assert.equal(findCapsuleDataFences(content).length, 2);
	});
});

describe("extract (code-fence)", () => {
	it("parses a JSON array fence", () => {
		const content = [
			"```capsule-data",
			'[{"ts":"06:00","mg_dl":95},{"ts":"06:05","mg_dl":98}]',
			"```",
		].join("\n");
		const sources = [resolved("[[r]]", "r.md", null, content)];
		const result = extract(sources, "code-fence");
		assert.equal(result.records.length, 2);
		assert.deepEqual(result.records[0], { ts: "06:00", mg_dl: 95 });
		assert.equal(result.errors.length, 0);
	});

	it("parses a CSV fence", () => {
		const content = [
			"```capsule-data:csv",
			"ts,mg_dl",
			"06:00,95",
			"06:05,98",
			"```",
		].join("\n");
		const sources = [resolved("[[r]]", "r.md", null, content)];
		const result = extract(sources, "code-fence");
		assert.equal(result.records.length, 2);
		assert.deepEqual(result.records[0], { ts: "06:00", mg_dl: "95" });
	});

	it("concatenates records across multiple fences", () => {
		const content = [
			"```capsule-data",
			'[{"v":1}]',
			"```",
			"",
			"```capsule-data",
			'[{"v":2},{"v":3}]',
			"```",
		].join("\n");
		const sources = [resolved("[[r]]", "r.md", null, content)];
		const result = extract(sources, "code-fence");
		assert.equal(result.records.length, 3);
		assert.deepEqual(
			result.records.map((r) => r.v),
			[1, 2, 3],
		);
	});

	it("errors per-source when no fence is present", () => {
		const sources = [resolved("[[r]]", "r.md", null, "No fences here.")];
		const result = extract(sources, "code-fence");
		assert.equal(result.records.length, 0);
		assert.equal(result.errors.length, 1);
		assert.match(result.errors[0].message, /no .*capsule-data.* fence/);
	});

	it("records a parse error per bad fence but keeps other records", () => {
		const content = [
			"```capsule-data",
			'[{"v":1}]',
			"```",
			"",
			"```capsule-data",
			"{ not json",
			"```",
		].join("\n");
		const sources = [resolved("[[r]]", "r.md", null, content)];
		const result = extract(sources, "code-fence");
		assert.equal(result.records.length, 1);
		assert.equal(result.errors.length, 1);
		assert.match(result.errors[0].message, /failed to parse/);
	});

	it("rejects non-array JSON", () => {
		const content = [
			"```capsule-data",
			'{"not": "an array"}',
			"```",
		].join("\n");
		const sources = [resolved("[[r]]", "r.md", null, content)];
		const result = extract(sources, "code-fence");
		assert.equal(result.records.length, 0);
		assert.equal(result.errors.length, 1);
		assert.match(result.errors[0].message, /must be an array/);
	});
});
