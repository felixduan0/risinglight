// Copyright 2023 RisingLight Project Authors. Licensed under Apache-2.0.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::vec::Vec;

use crate::catalog::{ColumnDesc, ColumnId, RootCatalog, TableRefId, DEFAULT_SCHEMA_NAME};
use crate::parser::{Ident, ObjectName, Statement};
use crate::types::{DataTypeKind, DataValue};

mod expr_visitor;
mod expression;
pub(crate) mod statement;
mod table_ref;

pub use self::expr_visitor::*;
pub use self::expression::*;
pub use self::statement::*;
pub use self::table_ref::*;

/// A bound SQL statement generated by the binder.
#[derive(Debug, PartialEq, Clone)]
pub enum BoundStatement {
    CreateTable(BoundCreateTable),
    Drop(BoundDrop),
    Insert(BoundInsert),
    Copy(BoundCopy),
    Select(Box<BoundSelect>),
    Explain(Box<BoundStatement>),
    Delete(Box<BoundDelete>),
}

/// The error type of bind operations.
#[derive(thiserror::Error, Debug, PartialEq, Eq)]
pub enum BindError {
    #[error("invalid database {0}")]
    InvalidDatabase(String),
    #[error("invalid schema {0}")]
    InvalidSchema(String),
    #[error("invalid table {0}")]
    InvalidTable(String),
    #[error("invalid column {0}")]
    InvalidColumn(String),
    #[error("duplicated table {0}")]
    DuplicatedTable(String),
    #[error("duplicated column {0}")]
    DuplicatedColumn(String),
    #[error("invalid expression: {0}")]
    InvalidExpression(String),
    #[error("not nullable column: {0}")]
    NotNullableColumn(String),
    #[error("binary operator types mismatch: {0} != {1}")]
    BinaryOpTypeMismatch(String, String),
    #[error("ambiguous column")]
    AmbiguousColumn,
    #[error("invalid table name: {0:?}")]
    InvalidTableName(Vec<Ident>),
    #[error("SQL not supported")]
    NotSupportedTSQL,
    #[error("invalid SQL")]
    InvalidSQL,
    #[error("cannot cast {0:?} to {1:?}")]
    CastError(DataValue, DataTypeKind),
    #[error("{0}")]
    BindFunctionError(String),
}

/// The context of binder execution.
#[derive(Debug, Default)]
struct BinderContext {
    regular_tables: HashMap<String, TableRefId>,
    // Mapping the table name to column names
    column_names: HashMap<String, HashSet<String>>,
    // Mapping table name to its column ids
    column_ids: HashMap<String, Vec<ColumnId>>,
    // Mapping table name to its column descrptions
    column_descs: HashMap<String, Vec<ColumnDesc>>,
    // Stores alias information
    aliases: Vec<String>,

    aliases_expressions: Vec<BoundExpr>,
}

/// The binder resolves all expressions referring to schema objects such as
/// tables or views with their column names and types.
pub struct Binder {
    catalog: Arc<RootCatalog>,
    context: BinderContext,
    upper_contexts: Vec<BinderContext>,
    base_table_refs: Vec<String>,
}

impl Binder {
    /// Create a new binder.
    pub fn new(catalog: Arc<RootCatalog>) -> Self {
        Binder {
            catalog,
            upper_contexts: Vec::new(),
            context: BinderContext::default(),
            base_table_refs: Vec::new(),
        }
    }

    fn push_context(&mut self) {
        let new_context = std::mem::take(&mut self.context);
        self.upper_contexts.push(new_context);
    }

    fn pop_context(&mut self) {
        let old_context = self.upper_contexts.pop();
        self.context = old_context.unwrap();
    }

    /// Bind a statement.
    pub fn bind(&mut self, stmt: &Statement) -> Result<BoundStatement, BindError> {
        match stmt {
            Statement::CreateTable { .. } => {
                Ok(BoundStatement::CreateTable(self.bind_create_table(stmt)?))
            }
            Statement::Drop { .. } => Ok(BoundStatement::Drop(self.bind_drop(stmt)?)),
            Statement::Insert { .. } => Ok(BoundStatement::Insert(self.bind_insert(stmt)?)),
            Statement::Delete { .. } => Ok(BoundStatement::Delete(self.bind_delete(stmt)?)),
            Statement::Copy { .. } => Ok(BoundStatement::Copy(self.bind_copy(stmt)?)),
            Statement::Query(query) => Ok(BoundStatement::Select(self.bind_select(query)?)),
            Statement::Explain { statement, .. } => {
                Ok(BoundStatement::Explain((self.bind(statement)?).into()))
            }
            Statement::ShowVariable { .. }
            | Statement::ShowCreate { .. }
            | Statement::ShowColumns { .. } => Err(BindError::NotSupportedTSQL),
            _ => Err(BindError::InvalidSQL),
        }
    }
}

/// Split an object name into `(schema name, table name)`.
fn split_name(name: &ObjectName) -> Result<(&str, &str), BindError> {
    Ok(match name.0.as_slice() {
        [table] => (DEFAULT_SCHEMA_NAME, &table.value),
        [schema, table] => (&schema.value, &table.value),
        _ => return Err(BindError::InvalidTableName(name.0.clone())),
    })
}

/// Convert an object name into lower case
fn lower_case_name(name: &ObjectName) -> ObjectName {
    ObjectName(
        name.0
            .iter()
            .map(|ident| Ident::new(ident.value.to_lowercase()))
            .collect::<Vec<_>>(),
    )
}
