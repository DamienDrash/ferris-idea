//! The integration tests of `idecraft-database` as one test program (ADR-0030 item 7): one
//! program to build and link instead of one per file. Each former file is a module here.

mod common;

mod agent_sessions;
mod cancel;
mod catalog;
mod connection_form;
mod connectors;
mod ddl;
mod duckdb;
mod edit;
mod er;
mod er_compare_live;
mod export;
mod introspect;
mod mariadb;
mod mysql;
mod oracle;
mod plan;
mod postgres;
mod refactor;
mod refactor_live;
mod reference_projects;
mod split_live;
mod split_properties;
mod sqlite;
mod sqlite_catalogue;
mod sqlserver;
mod table_editor;
mod tls;
mod tunnel;
