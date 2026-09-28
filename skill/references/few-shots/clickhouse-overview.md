====== ClickHouse--ClickHouse Repository Overview ======

===== Purpose =====
ClickHouse is an open-source, column-oriented database management system (DBMS) designed for real-time analytical processing (OLAP). The repository contains the complete source code for the ClickHouse server, client libraries, and tools, enabling high-performance data ingestion, storage, and querying at petabyte scale.

===== End-to-End Architecture =====

<mermaid>
graph TD
    Client[SQL Client] --> Parsers
    Parsers --> AST[Abstract Syntax Tree]
    AST --> Analyzer
    Analyzer --> QueryTree[Query Tree]
    QueryTree --> Query_Planning
    Query_Planning --> QueryPipeline[Query Pipeline]
    QueryPipeline --> Interpreters
    Interpreters --> Storage_Engine
    Storage_Engine --> IO_System
    IO_System --> Disk[Local/Remote Storage]

    Core_Engine --> |settings| Parsers
    Core_Engine --> |settings| Analyzer
    Core_Engine --> |settings| Query_Planning
    Core_Engine --> |settings| Interpreters
    Core_Engine --> |settings| Storage_Engine

    Access_Control --> |auth| Interpreters
    Data_Types --> |types| Parsers
    Data_Types --> |types| Analyzer
    Data_Types --> |types| Interpreters
    Data_Types --> |types| Storage_Engine
    Columns --> |columns| Data_Types
    Columns --> |columns| Storage_Engine
    Functions --> |functions| Analyzer
    Functions --> |functions| Interpreters
    Aggregate_Functions --> |aggs| Interpreters
    Common_Utilities --> |utils| All

    style Client fill:#fff,stroke:#333
    style Core_Engine fill:#f9f,stroke:#333,stroke-width:3px
    style Storage_Engine fill:#bbf,stroke:#333,stroke-width:2px
    style Interpreters fill:#bfb,stroke:#333,stroke-width:2px
</mermaid>

===== Core Modules Documentation =====

^ Module ^ Path ^ Purpose ^
|**Core_Engine** | ''src/Core'' | Central configuration, settings, and server-wide parameters. See [[repo:core_engine:start|Core Engine]] |
|**Interpreters** | ''src/Interpreters'' | Query execution context, expression evaluation, aggregation, and catalog management. See [[repo:interpreters:start|Interpreters]] |
|**Storage_Engine** | ''src/Storages'' | MergeTree family table engines, data parts, merges, mutations, and physical I/O. See [[repo:storage_engine:start|Storage Engine]] |
|**Query_Planning** | ''src/Processors/QueryPlan'' | Query plan construction, optimization passes, index selection, and parallel execution strategies. See [[repo:query_planning:start|Query Planning]] |
|**Data_Types** | ''src/DataTypes'' | Type system, serialization, LowCardinality, enums, and schema evolution. See [[repo:data_types:start|Data Types]] |
|**Functions** | ''src/Functions'' | Scalar and aggregate function registry, overload resolution, and runtime execution. See [[repo:functions:start|Functions]] |
|**Parsers** | ''src/Parsers'' | SQL lexer, grammar, AST construction, and query validation. See [[repo:parsers:start|Parsers]] |
|**IO_System** | ''src/IO'' | Buffered readers, seekers, connection timeouts, and data streaming. See [[repo:io_system:start|IO System]] |
|**Columns** | ''src/Columns'' | Columnar data structures, compression, SIMD operations, and memory management. See [[repo:columns:start|Columns]] |
|**Access_Control** | ''src/Access'' | Authentication, RBAC, row-level security, quotas, and LDAP integration. See [[repo:access_control:start|Access Control]] |
|**Query_Pipeline** | ''src/QueryPipeline'' | Pipeline orchestration, resource tracking, progress monitoring, and cancellation. See [[repo:query_pipeline:start|Query Pipeline]] |
|**Aggregate_Functions** | ''src/AggregateFunctions'' | Aggregate function factory, combinators, and compiled aggregation. See [[repo:aggregate_functions:start|Aggregate Functions]] |
|**Analyzer** | ''src/Analyzer'' | AST-to-Query-Tree conversion, semantic analysis, and optimization passes. See [[repo:analyzer:start|Analyzer]] |
|**Common_Utilities** | ''src/Common'' | DNS resolution, stack traces, thread pools, and shared utilities. See [[repo:common_utilities:start|Common Utilities]] |

===== Quick Start =====

  - **Build**: ''cmake -S . -B build && cmake --build build''
  - **Run**: ''build/programs/clickhouse-server --config-file=programs/server/config.xml''
  - **Connect**: ''clickhouse-client --query "SELECT version()"''

===== Key Features =====

  * **Columnar Storage**: Vectorized execution and compression
  * **Real-time Inserts**: Millions of rows per second ingestion
  * **SQL Support**: ANSI SQL plus powerful extensions
  * **Distributed Processing**: Sharding and replication out-of-the-box
  * **Data Formats**: CSV, JSON, Parquet, ORC, Avro, and more
  * **Integrations**: Kafka, S3, Hadoop, MySQL, PostgreSQL
  * **Monitoring**: Built-in metrics, query log, and system tables
