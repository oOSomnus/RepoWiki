# Reference style: complex implementation architecture module

A complex page may use different diagrams for different questions. One graph
shows composition; a sequence diagram shows how a concrete request moves
through the implementation.

```mermaid
sequenceDiagram
    participant Parser
    participant CreateQuery
    participant Columns
    participant Storage
    participant Formatter

    Parser->>CreateQuery: create DDL AST
    Parser->>Columns: parse columns and constraints
    Columns-->>CreateQuery: attach column list
    Parser->>Storage: parse engine and ordering
    Storage-->>CreateQuery: attach storage definition
    Formatter->>CreateQuery: format or serialize query
    CreateQuery-->>Formatter: formatted SQL
```

The page explains the responsibilities of the AST node, column container, and
storage definition before describing the parser-to-formatter lifecycle. It
uses concrete types and fields from the implementation instead of a generic
module summary.
