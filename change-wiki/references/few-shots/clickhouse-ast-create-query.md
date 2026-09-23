# AST_Create_Query Module Documentation

## Overview

The AST_Create_Query module is a core component of the ClickHouse SQL parser system, responsible for representing and handling CREATE, ATTACH, and related DDL (Data Definition Language) queries in Abstract Syntax Tree (AST) form. This module provides the foundational structures for parsing, storing, and formatting database object creation statements.

## Purpose and Core Functionality

The primary purpose of the AST_Create_Query module is to:

1. **Represent CREATE/ATTACH Statements**: Define AST nodes for various database object creation operations including tables, views, dictionaries, and databases
2. **Handle Complex DDL Syntax**: Support sophisticated CREATE statement syntax including storage engines, partitioning, indexing, and constraints
3. **Enable Query Manipulation**: Provide cloning and formatting capabilities for query transformation and serialization
4. **Support Multiple Object Types**: Handle different database objects through a unified interface

## Architecture and Component Relationships

### Core Components

#### 1. ASTColumnsElement
- **Purpose**: Wrapper class for individual column and constraint elements within CREATE statements
- **Key Features**:
  - Supports prefix-based formatting (e.g., "INDEX", "CONSTRAINT", "PROJECTION")
  - Provides cloning and formatting capabilities
  - Manages child element references through pointer manipulation

#### 2. ASTStorage
- **Purpose**: Represents storage engine configuration and table properties
- **Components**:
  - Engine specification (`ASTFunction *engine`)
  - Partitioning configuration (`IAST *partition_by`)
  - Primary key definition (`IAST *primary_key`)
  - Ordering configuration (`IAST *order_by`)
  - Sampling configuration (`IAST *sample_by`)
  - TTL settings (`IAST *ttl_table`)
  - Storage settings (`ASTSetQuery *settings`)

#### 3. ASTColumns
- **Purpose**: Container for column definitions, indices, constraints, and projections
- **Components**:
  - Column definitions (`ASTExpressionList *columns`)
  - Index definitions (`ASTExpressionList *indices`)
  - Constraint definitions (`ASTExpressionList *constraints`)
  - Projection definitions (`ASTExpressionList *projections`)
  - Primary key handling (`IAST *primary_key`, `IAST *primary_key_from_columns`)

#### 4. ASTCreateQuery
- **Purpose**: Main AST node representing CREATE/ATTACH statements
- **Key Features**:
  - Multi-object support (tables, views, dictionaries, databases)
  - Comprehensive flag system for different statement types
  - UUID management for replication and clustering
  - Target table support for materialized views
  - SQL security support for views

### Module Dependencies

```mermaid
graph TD
    AST_Create_Query[AST_Create_Query Module]
    
    subgraph "Parser Dependencies"
        ASTExpressionList[ASTExpressionList]
        ASTFunction[ASTFunction]
        ASTSelectWithUnionQuery[ASTSelectWithUnionQuery]
        CommonParsers[CommonParsers]
        CreateQueryUUIDs[CreateQueryUUIDs]
        ASTDictionary[ASTDictionary]
        ASTDictionaryAttributeDeclaration[ASTDictionaryAttributeDeclaration]
        ASTTableOverrides[ASTTableOverrides]
        ASTViewTargets[ASTViewTargets]
        ASTSQLSecurity[ASTSQLSecurity]
        ASTRefreshStrategy[ASTRefreshStrategy]
    end
    
    subgraph "Core Dependencies"
        StorageID[StorageID]
        QuoteString[quoteString]
        IOOperators[IO Operators]
        WriteBufferFromString[WriteBufferFromString]
    end
    
    AST_Create_Query --> ASTExpressionList
    AST_Create_Query --> ASTFunction
    AST_Create_Query --> ASTSelectWithUnionQuery
    AST_Create_Query --> CommonParsers
    AST_Create_Query --> CreateQueryUUIDs
    AST_Create_Query --> ASTDictionary
    AST_Create_Query --> ASTTableOverrides
    AST_Create_Query --> ASTViewTargets
    AST_Create_Query --> ASTSQLSecurity
    AST_Create_Query --> ASTRefreshStrategy
    AST_Create_Query --> StorageID
    AST_Create_Query --> QuoteString
    AST_Create_Query --> IOOperators
    AST_Create_Query --> WriteBufferFromString
```

## Data Flow and Processing

### Query Parsing Flow

```mermaid
sequenceDiagram
    participant Parser
    participant ASTCreateQuery
    participant ASTColumns
    participant ASTStorage
    participant Formatter
    
    Parser->>ASTCreateQuery: Create AST node
    Parser->>ASTCreateQuery: Set statement type flags
    Parser->>ASTCreateQuery: Parse table/database name
    
    alt Has columns
        Parser->>ASTColumns: Create columns container
        Parser->>ASTColumns: Parse column definitions
        Parser->>ASTColumns: Parse indices/constraints
        ASTColumns->>ASTCreateQuery: Set columns_list
    end
    
    alt Has storage definition
        Parser->>ASTStorage: Create storage node
        Parser->>ASTStorage: Parse engine specification
        Parser->>ASTStorage: Parse partition/order clauses
        ASTStorage->>ASTCreateQuery: Set storage
    end
    
    alt Has SELECT clause
        Parser->>ASTCreateQuery: Set select query
    end
    
    Formatter->>ASTCreateQuery: Format for output
    ASTCreateQuery->>Formatter: Return formatted SQL
```

### Object Type Handling

```mermaid
graph TD
    Start[CREATE Statement]
    
    Start --> CheckType{Object Type?}
    
    CheckType -->|Database| DatabasePath[Database Processing]
    CheckType -->|Table| TablePath[Table Processing]
    CheckType -->|View| ViewPath[View Processing]
    CheckType -->|Dictionary| DictionaryPath[Dictionary Processing]
    
    DatabasePath --> DatabaseFormat[Format Database Statement]
    TablePath --> TableFormat[Format Table Statement]
    ViewPath --> ViewFormat[Format View Statement]
    DictionaryPath --> DictionaryFormat[Format Dictionary Statement]
    
    DatabaseFormat --> End[End]
    TableFormat --> End
    ViewFormat --> End
    DictionaryFormat --> End
```

## Component Interactions

### ASTColumnsElement Usage

```mermaid
graph LR
    ASTColumns[ASTColumns]
    ASTColumnsElement[ASTColumnsElement]
    IAST[IAST Elements]
    
    ASTColumns -->|wraps| ASTColumnsElement
    ASTColumnsElement -->|contains| IAST
    
    subgraph "Element Types"
        Column[Column Definition]
        Index[Index Definition]
        Constraint[Constraint Definition]
        Projection[Projection Definition]
    end
    
    ASTColumnsElement -->|prefix=empty| Column
    ASTColumnsElement -->|prefix=INDEX| Index
    ASTColumnsElement -->|prefix=CONSTRAINT| Constraint
    ASTColumnsElement -->|prefix=PROJECTION| Projection
```

## Key Features and Capabilities

### 1. Multi-Object Support
The module handles various database objects through a unified interface:
- **Tables**: Standard tables with columns and storage engines
- **Views**: Ordinary, materialized, live, and window views
- **Dictionaries**: External data source dictionaries
- **Databases**: Database creation with storage engines

### 2. Advanced Storage Configuration
Supports complex storage engine configurations:
- Engine specification with parameters
- Partitioning strategies
- Primary key and ordering configurations
- TTL (Time To Live) settings
- Storage-specific settings

### 3. View Management
Comprehensive view support including:
- Multiple view types (ordinary, materialized, live, window)
- Target table specifications for materialized views
- SQL security configurations
- Refresh strategies for materialized views
- Parameterized view support

### 4. Clustering and Replication
Built-in support for distributed environments:
- UUID generation and management
- Cluster-aware formatting
- Replication configuration
- ON CLUSTER clause support

## Integration with Other Modules

### Related Modules
- **[Parsers](Parsers.md)**: Parent module providing base parsing infrastructure
- **[Interpreters](Interpreters.md)**: Processes AST nodes into executable operations
- **[Storage_Engine](Storage_Engine.md)**: Handles storage engine implementations
- **[Core_Engine](Core_Engine.md)**: Provides core system settings and utilities

### Usage Context
The AST_Create_Query module is typically used in:
1. **Query Parsing**: Converting SQL text to AST representation
2. **Query Analysis**: Examining CREATE statement structure
3. **Query Transformation**: Modifying or rewriting CREATE statements
4. **Query Serialization**: Converting AST back to SQL text
5. **Schema Management**: Database administration and migration tools

## Process Flow Examples

### CREATE TABLE Processing

```mermaid
flowchart TD
    A[CREATE TABLE Statement] --> B[Parse Table Name]
    B --> C{Has Columns?}
    C -->|Yes| D[Parse Column Definitions]
    C -->|No| E[Check AS Clause]
    
    D --> F[Parse Constraints/Indices]
    F --> G[Parse Storage Engine]
    
    G --> H{Has Engine?}
    H -->|Yes| I[Parse Engine Parameters]
    H -->|No| J[Use Default Engine]
    
    I --> K[Parse Table Options]
    J --> K
    E --> K
    
    K --> L[Build ASTCreateQuery]
    L --> M[Validate Statement]
    M --> N[Return AST Node]
```

### CREATE VIEW Processing

```mermaid
flowchart TD
    A[CREATE VIEW Statement] --> B[Parse View Name]
    B --> C{View Type?}
    C -->|Ordinary| D[Set is_ordinary_view]
    C -->|Materialized| E[Set is_materialized_view]
    C -->|Live| F[Set is_live_view]
    C -->|Window| G[Set is_window_view]
    
    D --> H[Parse Column Aliases]
    E --> I[Parse Target Table]
    F --> H
    G --> J[Parse Watermark Strategy]
    
    H --> K[Parse SELECT Query]
    I --> K
    J --> K
    
    K --> L[Build ASTCreateQuery]
    L --> M[Return AST Node]
```

## Error Handling and Validation

The module incorporates several validation mechanisms:
- **Type Checking**: Ensures correct object type combinations
- **Required Elements**: Validates mandatory components for each object type
- **Syntax Validation**: Checks SQL syntax compliance during formatting
- **UUID Consistency**: Validates UUID generation and management

## Performance Considerations

### Memory Management
- Uses shared pointers for automatic memory management
- Implements efficient cloning through selective copying
- Minimizes string allocations during formatting

### Processing Optimization
- Lazy evaluation of complex components
- Efficient tree traversal algorithms
- Optimized formatting for large statements

## Extension Points

The module provides several extension mechanisms:
- **Custom Storage Engines**: Through ASTStorage extension
- **New Object Types**: Via ASTCreateQuery inheritance
- **Additional Constraints**: Through ASTColumns modification
- **Formatting Options**: Via FormatSettings customization

This comprehensive design makes the AST_Create_Query module a robust foundation for handling complex DDL operations in ClickHouse while maintaining flexibility for future enhancements.
