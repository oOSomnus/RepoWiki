# Storage Engine Module

## 1. Introduction

The Storage Engine is the component responsible for managing the physical storage of data on disk. It defines how data is stored, retrieved, updated, and deleted. The primary implementation within this module is the `MergeTree` family of table engines, which are designed for high-performance data insertion and large-scale data analysis.

## 2. Architecture

The Storage Engine's architecture is centered around the `MergeTree` engine and its variants. The core class, `MergeTreeData`, manages the data parts, their metadata, and background processes like merging and mutation. `StorageMergeTree` and `StorageReplicatedMergeTree` are the main table engine implementations that build upon `MergeTreeData`.

```mermaid
graph TD
    subgraph Storage_Engine
        MergeTreeData
        StorageMergeTree
        StorageReplicatedMergeTree
        MergeTreeSettings
        MergeTreeDataMergerMutator
        MergeTreeDataSelectExecutor
        MergeTreePartsMover
    end

    Interpreters -- Executes Queries --> Storage_Engine
    Query_Planning -- Creates Query Plan --> Storage_Engine
    Storage_Engine -- Reads/Writes Data --> IO_System
    Storage_Engine -- Uses --> Data_Types
    Storage_Engine -- Uses --> Columns

    MergeTreeData -- Manages --> MergeTreeDataMergerMutator
    MergeTreeData -- Manages --> MergeTreeDataSelectExecutor
    MergeTreeData -- Manages --> MergeTreePartsMover
    StorageMergeTree -- Inherits from --> MergeTreeData
    StorageReplicatedMergeTree -- Inherits from --> MergeTreeData
    MergeTreeData -- Configured by --> MergeTreeSettings

    linkStyle 7 stroke:#ff0000,stroke-width:2px
    linkStyle 8 stroke:#ff0000,stroke-width:2px
    linkStyle 9 stroke:#ff0000,stroke-width:2px
```

### Component Dependencies

```mermaid
sequenceDiagram
    participant User
    participant Interpreters
    participant Query_Planning
    participant Storage_Engine
    participant IO_System

    User->>Interpreters: Executes a SELECT query
    Interpreters->>Query_Planning: Build query plan
    Query_Planning->>Storage_Engine: Read from storage
    Storage_Engine->>Storage_Engine: Select parts and marks to read (MergeTreeDataSelectExecutor)
    Storage_Engine->>IO_System: Read data from disk
    IO_System-->>Storage_Engine: Return data blocks
    Storage_Engine-->>Interpreters: Return data stream
    Interpreters-->>User: Return query result
```

## 3. Sub-modules

The Storage Engine is divided into several key sub-modules:

- **[MergeTree Engine](MergeTree_Engine.md)**: Implements the `StorageMergeTree` and `StorageReplicatedMergeTree` table engines, which are the primary entry points for data manipulation and querying.
- **[MergeTree Data](MergeTree_Data.md)**: The core data management layer for the MergeTree family. It handles data parts, background merging, mutations, and data selection.
- **[MergeTree Settings](MergeTree_Settings.md)**: Provides a comprehensive set of configurable parameters to fine-tune the behavior of MergeTree tables.
- **[Storage Utilities](Storage_Utilities.md)**: Contains helper components and data structures used across the storage engine.
