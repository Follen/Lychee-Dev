# Skill identity rename

Date: 2026-09-12. Previous implementation: `49ffe54`. Branch: `codex/remove-performance`.

The user requested the visible name **Lychee Dev skill**. The valid machine name and invocation are **`lychee-dev`** and **`$lychee-dev`**.

[Repository source](<../../../Lychee Dev skill/SKILL.md>) stays in `Lychee Dev skill/`. Frontmatter, heading, UI metadata, default prompt, root README translations, AGENTS.md and current investigation documentation use the new identity. The diagnostic reference has no old identity references and its behavior is unchanged.

Before migration, both old `wowdev` installations exactly matched all three repository files. Neither target `lychee-dev` path existed. Source/target parent directories and link status were checked before moving. Both independent directories were renamed and synchronized from the repository:

- `C:/Users/follen/.agents/skills/lychee-dev`
- `C:/Users/follen/.codex/skills/lychee-dev`

The two old `skills/wowdev` paths no longer exist; no alias or duplicate old entry was left. The previous backup and the removal validation record retain their historical names and hashes. The earlier record now explicitly links to this rename.

Validation: quick_validate.py passed for the repository and both new installations. Machine name, visible name, default invocation, exact file sets/content and absence of the two old paths were checked. Local Markdown links and current identity references were checked. No addon runtime, input UI, packaging behavior or game deployment changed, so the game test matrix was not rerun for this metadata/documentation change.

| File | SHA-256 (repository and both installations) |
| --- | --- |
| `SKILL.md` | `e158a278b91c04e96de31e434c5546186fd41fe79137198eb5cbe0043e838a31` |
| `agents/openai.yaml` | `08b33ae4635a359c6d8085a534dadcfa1a3b40cd62992485878d882dbb5e37fc` |
| `references/runtime-investigations.md` | `709adc841a2c4e45d93676348a4b480a8eef43ac67b8f3e7b215fb4454aaed23` |
