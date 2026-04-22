import { strict as assert } from "node:assert";
import { describe, it } from "node:test";

import { extract, parseSourceRef, ResolvedSource } from "./extraction";

function resolved(
	rawRef: string,
	path: string,
	frontmatter: Record<string, unknown> | null,
): ResolvedSource {
	return { rawRef, path, frontmatter };
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
