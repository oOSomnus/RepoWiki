# Architecture documentation few-shots

The four Markdown files below contain complete, unabridged reference articles
from the public CodeWiki ClickHouse demo. They are prompt examples, not facts
about the repository being documented. They were copied from commit
`9dc8cf8c41705960f2002f3489a6dc302c936114`:

- `clickhouse-overview.md` — [upstream overview](https://github.com/FSoft-AI4Code/codewiki-demo/blob/9dc8cf8c41705960f2002f3489a6dc302c936114/docs/ClickHouse--ClickHouse-docs/overview.md)
- `clickhouse-storage-engine.md` — [upstream Storage Engine](https://github.com/FSoft-AI4Code/codewiki-demo/blob/9dc8cf8c41705960f2002f3489a6dc302c936114/docs/ClickHouse--ClickHouse-docs/Storage_Engine.md)
- `clickhouse-query-pipeline.md` — [upstream Query Pipeline](https://github.com/FSoft-AI4Code/codewiki-demo/blob/9dc8cf8c41705960f2002f3489a6dc302c936114/docs/ClickHouse--ClickHouse-docs/Query_Pipeline.md)
- `clickhouse-ast-create-query.md` — [upstream AST Create Query](https://github.com/FSoft-AI4Code/codewiki-demo/blob/9dc8cf8c41705960f2002f3489a6dc302c936114/docs/ClickHouse--ClickHouse-docs/AST_Create_Query.md)

Pass complete, role-matched articles to writers: the overview to the
repository overview prompt; Storage Engine and Query Pipeline to parent/module
overview prompts; AST Create Query to leaf or complex implementation prompts.
Do not excerpt the reference body. Writers may learn its information density,
architecture explanation, and diagram discipline, but must derive all facts,
headings, nodes, and edges from the target repository.
