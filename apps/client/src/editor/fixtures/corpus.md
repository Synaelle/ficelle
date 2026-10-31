# Heading one

## Heading two with *emphasis* and `code`

### Heading three

#### Heading four

##### Heading five

###### Heading six

Plain paragraph with *emphasis*, **strong**, ***both***, ~~strike~~ and `inline code`.

Emphasis next to punctuation: *word*, **word**. "*quoted*" (*paren*) and intra**word**bold.

Escaped characters: \* not emphasis \*, \[not a link\], \_underscore\_, \# not heading, 1\. not list, \`not code\`, a\\b backslash.

\# Not a heading at line start

1\. Not a list at line start

\- Not a bullet either, and 5 \* 3 \> 2 with <span>inline html</span> and a &amp; entity.

French accents: « Élève à l'école », ça, où, garçon, cœur, naïve, Noël.

A [link](https://example.com) and a [link with title](https://example.com "Title") and an autolink <https://example.org>.

![An image](attachments/photo.png)

Accented paths: [lien accentué](notes/Été.md) and ![été](attachments/été.png).

![Alt text](attachments/img.png){width=320 wrap=left}

Text that flows around the floated image on the left.

![Centered](attachments/wide.png){width=600 wrap=center}

Line with trailing spaces hard break  
next line after spaces.

Line with backslash hard break\
next line after backslash.

- Bullet one
- Bullet two
  - Nested bullet
    - Deeply nested
- Bullet three

1. First
2. Second
   1. Nested ordered
   2. Another
3. Third

A paragraph separates the lists.

7. Starts at seven
8. Eight

- Item with code block:

  ```js
  const x = 1;
  ```

- Item after code

> A blockquote line.
> Second line of the quote.
>
> > Nested quote.

> - [ ] Task inside a blockquote
> - [x] Done task inside a blockquote

- [ ] Open task
- [x] Done task
  - [ ] Nested open task

```ts
function hello(name: string): string {
  return `Hello ${name}`;
}
```

```
no language fence
```

---

| Left | Center | Right | Default |
| :--- | :----: | ----: | ------- |
| a | b | c | d |
| pipe \| escaped | [[Note]] | **bold** | `code` |

Wiki links: [[Note]], [[Note|alias]], [[Note#Heading]] and [[Note#Heading|alias]].

Inline math $E = mc^2$ in a sentence.

$$
\int_0^1 x^2 \, dx = \frac{1}{3}
$$

> [!note] Callout title
> Callout body line one.
> Callout body with **bold**.

> [!warning]
> Warning without title.

Final paragraph.
