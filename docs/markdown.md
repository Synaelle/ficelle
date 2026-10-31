# Markdown in notes

Notes are plain Markdown files, shown formatted as you write. Everything standard works, with GitHub's additions: tables, task lists (`- [ ]`), strikethrough. The extras below are plain text in other apps, nothing is lost.

## Links between notes

| You write                 | You get                                   |
| ------------------------- | ----------------------------------------- |
| `[[Garden log]]`          | a link to the note titled Garden log      |
| `[[Garden log\|the log]]` | the same link, shown as "the log"         |
| `[[Garden log#2025]]`     | a link to the heading "2025" in that note |
| `[text](#2025)`           | a link to a heading in this note          |

Typing `[[` suggests titles, `#` after a title suggests its headings. Renaming a note updates the links to it. Each note lists the notes linking to it at its bottom.

## Images

Paste, drop or use the image button, then click an image to choose its layout and drag to resize. The layout is saved after the image, and other apps ignore it:

```markdown
![vines](attachments/vines.png){width=320 wrap=left}
```

`wrap` is `inline`, `left`, `right`, `center` or `full`.

## Callouts

```markdown
> [!note] Optional title
> Any text.
```

The kind is `note`, `tip`, `warning` or any other word.

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

They stay open while you write.

## Emoji

`:tada:` becomes 🎉, with GitHub's names for the common emoji. It can be turned off in Settings, Editor.

## Math

`$E = mc^2$` in a line, or a `$$` block. Shown as written for now. A dollar followed by a space or a digit stays text, so "$5" is safe.
