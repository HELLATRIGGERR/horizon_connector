# AE2 Guide document contract v1

This is the shared contract for the compiler, native reader and native scene
renderer. The compiler writes `assets/ae2/ae2guide/compiled-guide.json`; source
Markdown, SNBT and PNG files retain their original names under the same root.
The document is self-contained data and carries no executable code.

Top level: `{version: 1, guide_id: "ae2:guide", root: "index.md", pages,
item_index, categories, corpus}`. `pages` is keyed by guide-relative `.md` path.
Each page has `id`, `source_sha256`, `navigation` (source `title`, `parent`,
`position`, `icon`, `icon_components` as present), `categories`, `item_ids`,
`anchors`, and `nodes`. Keep source metadata that is not otherwise recognized
in `metadata`; do not discard it.

Each document node is an object with a `type` tag. Block nodes: `paragraph`,
`heading` (`level`, `anchor`), `list` (`ordered`, `start`, `items`),
`list_item`, `table` (`align`, `header`, `rows`), `quote`, `rule`, and
`code_block`. Inline nodes: `text` (`text`), `emphasis`, `strong`, `link`,
`image`, `strikethrough`, `code` (`text`), and `break`. Containers have `children` arrays.
Links retain `href` and a resolved `target`; images retain authored `src`,
`alt`, and a resolved `asset`. Lists and table cells retain their child nodes.

GuideME markup is represented as
`{type:"element",name,attrs,children,...}`. `name` is the exact source tag;
`attrs` retains attribute names and values (quoted values remain strings,
`{true}`/`{false}` are booleans), and nested children stay in source order.
Unknown tags or malformed markup are diagnostics, never silently dropped.
The current source also uses `a` (named anchors or links), `br`, and `div`.
`ItemLink`, `ItemImage`, `ItemIcon`, `BlockImage`, `RecipeFor`, and `Recipe`
also carry `resource_id`, using the `ae2:` default namespace. RecipeFor's ID
is an output item, Recipe's ID is a recipe identifier. Resolve both against
the currently loaded recipe data, not static fabricated recipes.
`ae2:ConfigValue` keeps its `name` attribute for live config lookup.

Every `GameScene` has a stable `scene_id` of `<page-id>#scene-<zero-based
source-order-ordinal>`, including nested scenes. The renderer receives that
element and its ordered children. `ImportStructure` carries both original
`attrs.src` and canonical `asset` (see below). Camera and Box/Line/Diamond
annotation tags preserve all source attributes and annotation children,
including nested scene nodes inside tooltips. Source `interactive` and zoom
are retained; the renderer owns camera state and hit testing per scene_id.

Guide-relative paths use POSIX `/`, never a host filesystem path. An imported
SNBT or Markdown image is resolved relative to its containing page directory,
normalized, and must remain inside the guide root; `asset` is e.g.
`assets/assemblies/tiny_me_system.snbt`. Resolve it under the pack's
`assets/ae2/ae2guide/` root. Markdown links to `.md` pages resolve the same way
and retain optional fragments; `#anchor` refers to the current page. Source
`navigation.parent` is guide-root-relative. External URLs remain explicit
external targets and are never used as a substitute for local guide content.

The bootstrap generates a local `compiled-sample.json` with actual compiled
navigation, links, recipe and config nodes, images, and nested scenes. The
compiler must validate all page links, all local images and all 139 scene
imports; preserve and report source-broken links rather than silently deleting
them. Corpus totals and diagnostics accompany the compiled result. The pinned
corpus currently has no diagnostics; the compiler fails if any appear or if
an authored component disappears during Markdown conversion.

The compiler is development-only; the native Rust runtime consumes the JSON
and source assets. Run `bun run scripts/prepare-content.ts` from the project
root to stage the pinned source under ignored `.local/guidebook`, install the
locked dependencies under `.local/guide-deps`, compile and verify the corpus,
and generate the local pack. `AE2_MARKED_MODULE` and `AE2_YAML_MODULE` may
instead point to both already installed module entry points for an offline
build. Verification checks source bytes, page/component order, scene imports,
the sample and a deterministic recompilation against the pinned corpus.
