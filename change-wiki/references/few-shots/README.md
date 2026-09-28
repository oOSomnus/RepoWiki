# Architecture documentation few-shots

These four reference files are complete prompt examples written in native DokuWiki page syntax.
The source pages were adapted from the public CodeWiki
ClickHouse demo at commit `9dc8cf8c41705960f2002f3489a6dc302c936114`. Their
filenames identify bundled prompt references; they are not output page paths.

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
