# Markdown in notes

Notes are Markdown files with GitHub's tables, task lists and strikethrough, plus the extras below.

## Links between notes

| You write                 | You get                            |
| ------------------------- | ---------------------------------- |
| `[[Garden log]]`          | link to the note titled Garden log |
| `[[Garden log\|the log]]` | same link, shown as "the log"      |
| `[[Garden log#2025]]`     | link to its heading "2025"         |
| `[text](#2025)`           | link to a heading in this note     |

Renaming a note updates the links to it.

## Images

```markdown
![vines](attachments/vines.png){width=320 wrap=left}
```

`wrap` is `inline`, `left`, `right`, `center` or `full`.

## Callouts

```markdown
> [!note] Optional title
> Any text.
```

## Footnotes

```markdown
A sentence with a note.[^1]

[^1]: The note itself.
```

## Folded sections

```markdown
<details>
<summary>Title</summary>

Anything.

</details>
```

## Emoji

`:tada:` becomes 🎉.

## Math

`$E = mc^2$` inline or a `$$` block, shown as written for now. "$5" stays text.
