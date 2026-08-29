/**
 * Writes the prelude reference pages from `crates/stdlib/prelude.d.lis`. PAGES decides the
 * split, and the parser throws on a declaration no page covers rather than dropping it.
 */
import { readFileSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SITE = dirname(dirname(fileURLToPath(import.meta.url)));
const SOURCE = join(SITE, "..", "crates", "stdlib", "prelude.d.lis");
// A test file sees this one on top of the prelude proper, so it is parsed separately.
const TARGET = join(SITE, "src", "content", "docs", "prelude");

const FENCE = "```";
// Inlined because a generated page cannot mount a component.
const SEE =
  '<svg class="see-icon" aria-hidden="true" viewBox="0 0 24 24" fill="currentColor"><path d="M21.17 2.06A13.1 13.1 0 0 0 19 1.87a12.94 12.94 0 0 0-7 2.05 12.94 12.94 0 0 0-7-2 13.1 13.1 0 0 0-2.17.19 1 1 0 0 0-.83 1v12a1 1 0 0 0 1.17 1 10.9 10.9 0 0 1 8.25 1.91l.12.07h.11a.91.91 0 0 0 .7 0h.11l.12-.07A10.899 10.899 0 0 1 20.83 16 1 1 0 0 0 22 15V3a1 1 0 0 0-.83-.94ZM11 15.35a12.87 12.87 0 0 0-6-1.48H4v-10c.333-.02.667-.02 1 0a10.86 10.86 0 0 1 6 1.8v9.68Zm9-1.44h-1a12.87 12.87 0 0 0-6 1.48V5.67a10.86 10.86 0 0 1 6-1.8c.333-.02.667-.02 1 0v10.04Zm1.17 4.15a13.098 13.098 0 0 0-2.17-.19 12.94 12.94 0 0 0-7 2.05 12.94 12.94 0 0 0-7-2.05c-.727.003-1.453.066-2.17.19A1 1 0 0 0 2 19.21a1 1 0 0 0 1.17.79 10.9 10.9 0 0 1 8.25 1.91 1 1 0 0 0 1.16 0A10.9 10.9 0 0 1 20.83 20a1 1 0 0 0 1.17-.79 1 1 0 0 0-.83-1.15Z"/></svg>';
const code = (text) => "`" + text + "`";

const PAGES = [
  {
    slug: "numerics",
    title: "Numerics",
    description: "The built-in integer, float and complex types",
    intro: "All numeric types in Lisette map 1:1 to their Go counterparts.",
    outro: [
      "## Adaptation",
      "",
      "Numerics can adapt to the expected type where the context is unambiguous.",
      "",
      "```lisette",
      "// !callout-right `int`",
      "let x = 42",
      "// !callout-right `int64`, literal `42` adapts to annotated type",
      "let y: int64 = 42",
      "// !callout-right `float64`",
      "let z = 3.14",
      "// !callout-right `float32`, literal `3.14` adapts to annotated type",
      "let w: float32 = 3.14",
      "```",
      "",
      SEE + "See [Type conversion](/docs/types/#type-conversion) for converting between numerics",
    ].join("\n"),
    table: {},
    sections: [],
  },
  {
    slug: "booleans",
    title: "Boolean",
    description: "The boolean type",
    noLink: true,
    sections: ["bool"],
  },
  {
    slug: "strings",
    title: "String",
    description: "The string type and its methods",
    note: [
      "Strings are immutable. Indexing and slicing one means picking a unit: `byte_at()` and",
      "`bytes()` count bytes, while `rune_at()` and `substring()` count runes. `length()` counts",
      "bytes, so it does not agree with `substring()` on text outside ASCII.",
      "",
      SEE + "See [Lexemes](/docs/lexemes/#strings) for string literals",
    ].join("\n"),
    sections: ["string"],
  },
  {
    slug: "option",
    title: "Option",
    description: "The type for a value that may be absent",
    noLink: true,
    sections: ["Option"],
  },
  {
    slug: "result",
    title: "Result",
    description: "The type for an operation that can fail",
    noLink: true,
    sections: ["Result"],
  },
  {
    slug: "partial",
    title: "Partial",
    description: "The type for a value and an error that are both meaningful",
    noLink: true,
    sections: ["Partial"],
  },
  {
    slug: "array",
    title: "Array",
    description: "The fixed-size sequence type",
    note: SEE + "Compare to [Slice](/docs/prelude/slice/)",
    sections: ["Array"],
  },
  {
    slug: "slice",
    title: "Slice",
    description: "The growable sequence type",
    note: SEE + "Compare to [Array](/docs/prelude/array/)",
    sections: ["Slice"],
  },
  {
    slug: "map",
    title: "Map",
    description: "The key-to-value type",
    noLink: true,
    sections: ["Map"],
  },
  {
    slug: "ranges",
    title: "Ranges",
    description: "The five range types a `..` expression builds",
    intro: [
      "Build a range using the `..` and `..=` operators, typically for looping or indexing.",
      "",
      SEE + "See [range operators](/docs/operators/#range)",
    ].join("\n"),
    sections: ["Range", "RangeInclusive", "RangeFrom", "RangeTo", "RangeToInclusive"],
  },
  {
    slug: "channels",
    title: "Channels",
    description: "Channels and their sending and receiving halves",
    intro: SEE + "See [Concurrency](/docs/concurrency/)",
    sections: ["Channel", "Sender", "Receiver"],
  },
  {
    slug: "ref",
    title: "Ref",
    description: "The pointer type",
    note: SEE + "See [References](/docs/references/)",
    sections: ["Ref"],
  },
  {
    slug: "functions",
    title: "Functions",
    description: "The functions callable without an import",
    noLink: true,
    sections: [],
    functions: ["assert_type", "complex", "imaginary", "max", "min", "panic", "real"],
    order: ["assert_type", "complex", "imaginary", "max", "min", "panic", "real"],
  },
  {
    slug: "types",
    title: "Types",
    description: "The types that carry meanings Lisette has no syntax for",
    noLink: true,
    sections: ["PanicValue", "Never", "Unit", "Unknown", "VarArgs", "error"],
    order: ["PanicValue", "Never", "Unit", "Unknown", "VarArgs", "error"],
  },
  {
    slug: "constraints",
    title: "Constraints",
    description: "The two markers usable as type-parameter bounds",
    noLink: true,
    sections: ["Comparable", "Ordered"],
  },
];

/** Declared but left undocumented, reached only through the method that returns it. */
const HIDDEN = new Set(["EnumeratedSlice"]);

const OWNER = new Map(
  PAGES.flatMap((page) =>
    [...page.sections, ...(page.functions ?? [])].map((name) => [name, page.slug]),
  ),
);

const NUMERICS = [
  "int",
  "int8",
  "int16",
  "int32",
  "int64",
  "rune",
  "uint",
  "uint8",
  "uint16",
  "uint32",
  "uint64",
  "uintptr",
  "byte",
  "float32",
  "float64",
  "complex64",
  "complex128",
];

const TYPE = /^(?:pub\s+)?type\s+(\w+)(?:<(.+)>)?$/;
const ENUM = /^enum\s+(\w+)(?:<(.+)>)?\s*\{$/;
const STRUCT = /^(?:pub\s+)?struct\s+(\w+)(?:<(.+)>)?\s*\{(\}?)$/;
const INTERFACE = /^interface\s+(\w+)(?:<(.+)>)?\s*\{$/;
const IMPL = /^impl(?:<(.+?)>)?\s+(\w+)(?:<(.+)>)?\s*\{$/;
const METHOD = /^(?:pub\s+)?fn\s+(\w+)/;

/** Splits `T, Map<K, V>` on the commas that are not inside angle brackets. */
const splitTopLevel = (text) => {
  const parts = [];
  let depth = 0;
  let current = "";
  for (const character of text) {
    if (character === "<") depth += 1;
    if (character === ">") depth -= 1;
    if (character === "," && depth === 0) {
      parts.push(current.trim());
      current = "";
      continue;
    }
    current += character;
  }
  if (current.trim() !== "") parts.push(current.trim());
  return parts;
};

/** True for `impl<T> Option<T>`, false for a specialization such as `impl Slice<string>`. */
const coversEveryInstance = (params, args) => {
  const declared = params ? splitTopLevel(params).map((part) => part.split(":")[0].trim()) : [];
  const supplied = args ? splitTopLevel(args) : [];
  return declared.length === supplied.length && declared.every((part, at) => part === supplied[at]);
};

/** Splits a doc comment into its prose and its trailing `Example:` block. */
const docComment = (lines) => {
  const at = lines.findIndex((line) => line.trim() === "Example:");
  const prose = (at === -1 ? lines : lines.slice(0, at)).join("\n").trim();
  if (at === -1) return { text: prose, example: null };

  const body = lines.slice(at + 1);
  const indent = Math.min(
    ...body.filter((line) => line.trim() !== "").map((line) => line.length - line.trimStart().length),
  );
  return { text: prose, example: body.map((line) => line.slice(indent)).join("\n").trim() };
};

const parse = (source) => {
  const lines = source.split("\n");
  const items = new Map();
  let doc = [];
  let attributes = [];
  let at = 0;

  /** Consumes the lines up to the closing brace of the block just opened. */
  const readBody = () => {
    const body = [];
    while (at < lines.length && lines[at].trim() !== "}") {
      body.push(lines[at].trim());
      at += 1;
    }
    at += 1;
    return body;
  };

  const take = () => {
    const taken = { doc: docComment(doc), attributes };
    doc = [];
    attributes = [];
    return taken;
  };

  const add = (item) => {
    if (items.has(item.name)) throw new Error(`prelude: ${item.name} is declared twice`);
    items.set(item.name, { members: [], declaration: null, ...item });
  };

  while (at < lines.length) {
    const line = lines[at].trim();
    const opensAt = at;
    at += 1;

    if (line.startsWith("///")) {
      doc.push(line.slice(3).replace(/^ /, ""));
      continue;
    }
    if (line.startsWith("#[")) {
      attributes.push(line);
      continue;
    }
    if (line === "" || line.startsWith("//")) {
      doc = [];
      attributes = [];
      continue;
    }

    if (TYPE.test(line)) {
      const [, name, params] = line.match(TYPE);
      add({ kind: "type", name, params, ...take() });
      continue;
    }

    if (ENUM.test(line) || INTERFACE.test(line) || STRUCT.test(line)) {
      const pattern = ENUM.test(line) ? ENUM : INTERFACE.test(line) ? INTERFACE : STRUCT;
      const [, name, params, closed] = line.match(pattern);
      if (!closed) readBody();
      const declaration = lines.slice(opensAt, at).join("\n").trim();
      add({ kind: "block", name, params, declaration, ...take() });
      continue;
    }

    if (IMPL.test(line)) {
      const [, params, name, args] = line.match(IMPL);
      const target = args ? name + "<" + args + ">" : name;
      const everyInstance = coversEveryInstance(params, args);
      const owner = items.get(name);
      if (!owner) throw new Error(`prelude: impl ${target} has no matching declaration`);

      let memberDoc = [];
      for (const entry of readBody()) {
        if (entry.startsWith("///")) {
          memberDoc.push(entry.slice(3).replace(/^ /, ""));
          continue;
        }
        if (entry === "") {
          memberDoc = [];
          continue;
        }
        if (!METHOD.test(entry)) throw new Error(`prelude: unparsed member ${code(entry)}`);
        owner.members.push({
          name: entry.match(METHOD)[1],
          signature: entry.replace(/^pub\s+/, ""),
          doc: docComment(memberDoc),
          implementation: line.replace(/\s*\{$/, ""),
          availableOn: everyInstance ? null : target,
        });
        memberDoc = [];
      }
      take();
      continue;
    }

    if (METHOD.test(line)) {
      const { doc: parsed, attributes: found } = take();
      add({
        kind: "function",
        name: line.match(METHOD)[1],
        // Attributes say how a declaration emits rather than how it is called, so they are dropped.
        signature: line.replace(/^pub\s+/, ""),
        doc: parsed,
      });
      continue;
    }

    throw new Error(`prelude: unparsed line ${at}: ${code(line)}`);
  }

  return items;
};

const fenced = (body, meta) => [FENCE + "lisette" + (meta ? " " + meta : ""), body, FENCE, ""];

const BARE_HEADING = new Set(["VarArgs"]);
const HEADING_SUFFIX = new Map([["error", "interface"]]);

const heading = (item) =>
  item.params && !BARE_HEADING.has(item.name) ? item.name + "<" + item.params + ">" : item.name;

const headingMarkup = (item) => {
  const suffix = HEADING_SUFFIX.get(item.name);
  return code(heading(item)) + (suffix ? " " + suffix : "");
};

const renderSignature = ({ label, signature, doc, implementation, example }, level) => {
  // Wrapped in its impl block, as the source writes it, so `self` is not read as a parameter of a free function.
  const declaration = implementation
    ? [implementation + " {", "  " + signature, "}"].join("\n")
    : signature;

  return [
    "#".repeat(level) + " " + code(label + "()"),
    "",
    ...(doc.text ? [doc.text, ""] : []),
    ...fenced(declaration),
    ...(example === null ? [] : fenced(example ?? doc.example)),
  ];
};

// An explicit `null` must survive the lookup, so this tests for the key rather than for a value.
const renderMembers = (item, level, examples = {}) =>
  item.members.flatMap((member) => {
    const label = item.name + "." + member.name;
    return renderSignature(
      { ...member, label, example: label in examples ? examples[label] : undefined },
      level,
    );
  });

const renderSection = (item, level, examples) => [
  "#".repeat(level) + " " + headingMarkup(item),
  "",
  ...(item.doc.text ? [item.doc.text, ""] : []),
  ...(item.declaration && !/\{\s*\}$/.test(item.declaration) ? fenced(item.declaration) : []),
  ...(item.doc.example ? fenced(item.doc.example) : []),
  ...renderMembers(item, level + 1, examples),
];

const renderSoleType = (item, note, examples) => [
  ...(item.doc.text ? [item.doc.text, ""] : []),
  ...(item.declaration && !/\{\s*\}$/.test(item.declaration) ? fenced(item.declaration) : []),
  ...(item.doc.example ? fenced(item.doc.example) : []),
  ...(note ? [note, ""] : []),
  ...renderMembers(item, 2, examples),
];

/* The final period is dropped, since a table cell is not a sentence. */
const renderTable = (page, names, items) => {
  const target = (name) => {
    if (page.sections.includes(name)) return "#" + name.toLowerCase();
    if (OWNER.has(name)) return "/docs/prelude/" + OWNER.get(name);
    return null;
  };

  return [
    ...(page.table.heading ? ["## " + page.table.heading, ""] : []),
    "| Type | Description |",
    "| ---- | ----------- |",
    ...names.map((name) => {
      const item = items.get(name);
      const href = target(name);
      const label = href ? "[" + code(name) + "](" + href + ")" : code(name);
      const description = item.doc.text.replace(/\n/g, " ").replace(/\.$/, "");
      return "| " + label + " | " + description + " |";
    }),
    "",
  ];
};

const isSoleType = (page) => page.sections.length === 1 && !page.table && !page.functions;

const render = (page, items) => {
  const body = [
    "---",
    'title: "' + page.title + '"',
    'description: "' + page.description + '"',
    "---",
    "",
    ...(isSoleType(page)
      ? renderSoleType(items.get(page.sections[0]), page.note, page.examples)
      : [
          page.intro,
          "",
          ...(page.table ? renderTable(page, NUMERICS, items) : []),
          ...chapters(page).flatMap((name) =>
            (page.functions ?? []).includes(name)
              ? renderSignature({ ...items.get(name), label: name }, 2)
              : renderSection(items.get(name), 2, page.examples),
          ),
          ...(page.outro ? [page.outro, ""] : []),
        ]),
  ];
  return body.join("\n").replace(/\n{3,}/g, "\n\n").trimEnd() + "\n";
};

const chapters = (page) => page.order ?? [...page.sections, ...(page.functions ?? [])];

const checkCoverage = (items, pages) => {
  const owned = pages.flatMap((page) => [...page.sections, ...(page.functions ?? [])]);
  const listed = pages.flatMap((page) => (page.table ? NUMERICS : []));

  const unknown = [...owned, ...listed].filter((name) => !items.has(name));
  if (unknown.length) throw new Error(`prelude: no such declaration: ${unknown.join(", ")}`);

  const counted = new Map();
  for (const name of owned) counted.set(name, (counted.get(name) ?? 0) + 1);

  const twice = [...counted].filter(([, count]) => count > 1).map(([name]) => name);
  if (twice.length) throw new Error(`prelude: owned by two pages: ${twice.join(", ")}`);

  const unlinked = pages
    .filter((page) => !page.noLink)
    .filter((page) => !/(?:See|Compare to) \[/.test((page.intro ?? "") + (page.note ?? "") + (page.outro ?? "")))
    .map((page) => page.slug);
  if (unlinked.length) throw new Error(`prelude: no chapter link on: ${unlinked.join(", ")}`);

  for (const page of pages.filter((entry) => entry.order)) {
    const declared = [...page.sections, ...(page.functions ?? [])];
    const same =
      declared.length === page.order.length && declared.every((name) => page.order.includes(name));
    if (!same) throw new Error(`prelude: ${page.slug} order does not match its declarations`);
  }

  const covered = new Set([...owned, ...listed, ...HIDDEN]);
  const missing = [...items.keys()].filter((name) => !covered.has(name));
  if (missing.length) throw new Error(`prelude: no page covers: ${missing.join(", ")}`);
};

const items = new Map(parse(readFileSync(SOURCE, "utf8")));
checkCoverage(items, PAGES);

// Removed wholesale, so a page dropped from PAGES leaves no orphan behind.
rmSync(TARGET, { recursive: true, force: true });
mkdirSync(TARGET, { recursive: true });

for (const page of PAGES) writeFileSync(join(TARGET, page.slug + ".md"), render(page, items));

const shown = [...items.values()].filter((item) => !HIDDEN.has(item.name));
const members = shown.reduce((total, item) => total + item.members.length, 0);
const hidden = HIDDEN.size ? ` (hidden: ${[...HIDDEN].join(", ")})` : "";
console.log(
  `prelude: wrote ${shown.length} declarations and ${members} methods to ${PAGES.length} pages${hidden}`,
);
