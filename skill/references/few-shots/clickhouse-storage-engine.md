# Reference style: parent architecture module

A parent page explains why its children form one subsystem and how the
subsystem interacts with neighboring modules.

```mermaid
flowchart TD
    Interpreters --> StorageEngine[Storage Engine]
    Planning[Query Planning] --> StorageEngine
    StorageEngine --> IO[IO System]
    StorageEngine --> Types[Data Types]
    StorageEngine --> Columns

    StorageEngine --> MergeTree[MergeTree Engine]
    MergeTree --> Parts[Data Parts]
    MergeTree --> Merges[Background Merges]
    MergeTree --> Reads[Read Executor]
```

The page then separates the external query path from the internal ownership
of parts, merges, reads, and settings. Child links are accompanied by a
sentence explaining how each child contributes to the parent responsibility;
the parent does not repeat all child implementation details.
