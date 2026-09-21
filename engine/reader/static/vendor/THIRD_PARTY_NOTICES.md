# Reader vendor notices

The standalone reader bundles the following browser assets so it can run
without a network connection. Versions are pinned to the files checked into
this directory.

| Asset | Package | Version | License |
| --- | --- | --- | --- |
| `marked.min.js` | `marked` | 11.0.0 | MIT |
| `marked-gfm-heading-id.min.js` | `marked-gfm-heading-id` | 3.1.3 | MIT |
| `mermaid.min.js` | `mermaid` | 11.9.0 | MIT (the distribution also bundles its own dependencies) |
| `highlight.min.js`, `highlight-*.css` | `@highlightjs/cdn-assets` / `highlight.js` | 11.9.0 | BSD-3-Clause |
| `purify.min.js` | `dompurify` | 3.2.6 | Apache-2.0 or MPL-2.0 |

Upstream project pages and complete license texts:

- [marked](https://github.com/markedjs/marked)
- [marked-gfm-heading-id](https://github.com/markedjs/marked-gfm-heading-id)
- [Mermaid](https://github.com/mermaid-js/mermaid)
- [highlight.js](https://github.com/highlightjs/highlight.js)
- [DOMPurify](https://github.com/cure53/DOMPurify)

These files are vendored for the RepoWiki reader only; the generated Skill
package remains independent of the reader and does not include them.
