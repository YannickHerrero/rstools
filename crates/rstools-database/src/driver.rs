use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls, Row};

// ── Types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct TableInfo {
    pub schema: String,
    pub name: String,
    pub row_count: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct ColumnInfo {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub is_primary_key: bool,
}

#[derive(Debug, Clone)]
pub struct QueryResult {
    pub columns: Vec<ColumnInfo>,
    pub rows: Vec<Vec<String>>,
    pub total_count: i64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct QueryFilter {
    pub column: String,
    pub operator: FilterOp,
    pub value: String,
}

#[derive(Debug, Clone)]
pub enum FilterOp {
    Contains,
    Equals,
    NotEquals,
    GreaterThan,
    LessThan,
}

#[derive(Debug, Clone)]
pub struct QueryParams {
    pub table: String,
    pub schema: String,
    pub offset: usize,
    pub limit: usize,
    pub sort_column: Option<String>,
    pub sort_direction: SortDirection,
    pub filters: Vec<QueryFilter>,
}

// ── Driver ──────────────────────────────────────────────────────────

pub struct PgDriver {
    client: Client,
    _connection_handle: JoinHandle<()>,
}

impl PgDriver {
    pub async fn connect(
        host: &str,
        port: u16,
        database: &str,
        user: &str,
        password: &str,
        ssl: bool,
    ) -> Result<Self> {
        let conn_string = format!(
            "host={} port={} dbname={} user={} password={}{}",
            host,
            port,
            database,
            user,
            password,
            if ssl { " sslmode=require" } else { "" }
        );

        if ssl {
            Self::connect_with_tls(&conn_string).await
        } else {
            Self::connect_no_tls(&conn_string).await
        }
    }

    async fn connect_no_tls(conn_string: &str) -> Result<Self> {
        let (client, connection) = tokio_postgres::connect(conn_string, NoTls)
            .await
            .context("Failed to connect to PostgreSQL")?;

        let handle = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("PostgreSQL connection error: {e}");
            }
        });

        Ok(Self {
            client,
            _connection_handle: handle,
        })
    }

    async fn connect_with_tls(conn_string: &str) -> Result<Self> {
        let config = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAllVerifier))
            .with_no_client_auth();

        let connector = tokio_postgres_rustls::MakeRustlsConnect::new(config);

        let (client, connection) = tokio_postgres::connect(conn_string, connector)
            .await
            .context("Failed to connect to PostgreSQL with TLS")?;

        let handle = tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("PostgreSQL connection error: {e}");
            }
        });

        Ok(Self {
            client,
            _connection_handle: handle,
        })
    }

    pub async fn server_version(&self) -> Result<String> {
        let row = self
            .client
            .query_one("SELECT version()", &[])
            .await
            .context("Failed to get server version")?;
        let version: String = row.get(0);
        Ok(version)
    }

    pub async fn get_tables(&self) -> Result<Vec<TableInfo>> {
        let rows = self
            .client
            .query(
                "SELECT schemaname, tablename
                 FROM pg_catalog.pg_tables
                 WHERE schemaname NOT IN ('pg_catalog', 'information_schema')
                 ORDER BY schemaname, tablename",
                &[],
            )
            .await
            .context("Failed to get tables")?;

        let mut tables = Vec::with_capacity(rows.len());
        for row in &rows {
            let schema: String = row.get(0);
            let name: String = row.get(1);

            // Get approximate row count from pg_class for performance
            let count_row = self
                .client
                .query_opt(
                    "SELECT reltuples::bigint
                     FROM pg_class c
                     JOIN pg_namespace n ON n.oid = c.relnamespace
                     WHERE n.nspname = $1 AND c.relname = $2",
                    &[&schema, &name],
                )
                .await
                .ok()
                .flatten();

            let row_count = count_row.and_then(|r| r.get::<_, Option<i64>>(0));

            tables.push(TableInfo {
                schema,
                name,
                row_count,
            });
        }

        Ok(tables)
    }

    pub async fn get_columns(&self, schema: &str, table: &str) -> Result<Vec<ColumnInfo>> {
        let rows = self
            .client
            .query(
                "SELECT
                    c.column_name,
                    c.data_type,
                    c.is_nullable,
                    CASE WHEN pk.column_name IS NOT NULL THEN true ELSE false END as is_pk
                 FROM information_schema.columns c
                 LEFT JOIN (
                     SELECT ku.column_name
                     FROM information_schema.table_constraints tc
                     JOIN information_schema.key_column_usage ku
                         ON tc.constraint_name = ku.constraint_name
                         AND tc.table_schema = ku.table_schema
                     WHERE tc.constraint_type = 'PRIMARY KEY'
                         AND tc.table_schema = $1
                         AND tc.table_name = $2
                 ) pk ON c.column_name = pk.column_name
                 WHERE c.table_schema = $1 AND c.table_name = $2
                 ORDER BY c.ordinal_position",
                &[&schema, &table],
            )
            .await
            .context("Failed to get columns")?;

        let columns = rows
            .iter()
            .map(|row| {
                let name: String = row.get(0);
                let data_type: String = row.get(1);
                let nullable_str: String = row.get(2);
                let is_pk: bool = row.get(3);

                ColumnInfo {
                    name,
                    data_type,
                    nullable: nullable_str == "YES",
                    is_primary_key: is_pk,
                }
            })
            .collect();

        Ok(columns)
    }

    pub async fn query(&self, params: &QueryParams) -> Result<QueryResult> {
        let qualified_table = format!(
            "{}.{}",
            quote_ident(&params.schema),
            quote_ident(&params.table)
        );

        // Build WHERE clause from filters
        let (where_clause, filter_values) = build_where_clause(&params.filters);

        // Get total count
        let count_sql = format!("SELECT COUNT(*) FROM {qualified_table}{where_clause}");
        let count_row = self
            .client
            .query_one(&count_sql, &filter_values_as_refs(&filter_values))
            .await
            .context("Failed to count rows")?;
        let total_count: i64 = count_row.get(0);

        // Build ORDER BY
        let order_clause = if let Some(ref col) = params.sort_column {
            let dir = match params.sort_direction {
                SortDirection::Asc => "ASC",
                SortDirection::Desc => "DESC",
            };
            format!(" ORDER BY {} {dir}", quote_ident(col))
        } else {
            String::new()
        };

        let offset = params.offset;
        let limit = params.limit;

        let data_sql = format!(
            "SELECT * FROM {qualified_table}{where_clause}{order_clause} LIMIT {limit} OFFSET {offset}"
        );

        let rows = self
            .client
            .query(&data_sql, &filter_values_as_refs(&filter_values))
            .await
            .context("Failed to query table data")?;

        // Get column info from the result set
        let columns = if let Some(first_row) = rows.first() {
            first_row
                .columns()
                .iter()
                .map(|col| ColumnInfo {
                    name: col.name().to_string(),
                    data_type: col.type_().name().to_string(),
                    nullable: true,
                    is_primary_key: false,
                })
                .collect()
        } else {
            Vec::new()
        };

        // Convert all values to strings for display
        let string_rows: Vec<Vec<String>> = rows.iter().map(|row| row_to_strings(row)).collect();

        Ok(QueryResult {
            columns,
            rows: string_rows,
            total_count,
        })
    }
}

// ── TLS verifier ────────────────────────────────────────────────────

/// Accept all server certificates (equivalent to sslmode=require without verify).
/// This is appropriate for development and when the user explicitly opts into SSL.
#[derive(Debug)]
struct AcceptAllVerifier;

impl rustls::client::danger::ServerCertVerifier for AcceptAllVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn quote_ident(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

fn build_where_clause(filters: &[QueryFilter]) -> (String, Vec<String>) {
    if filters.is_empty() {
        return (String::new(), Vec::new());
    }

    let mut parts = Vec::new();
    let mut values = Vec::new();

    for (i, f) in filters.iter().enumerate() {
        let param_idx = i + 1;
        let col = quote_ident(&f.column);
        match f.operator {
            FilterOp::Contains => {
                parts.push(format!("{col}::text ILIKE ${param_idx}"));
                values.push(format!("%{}%", f.value));
            }
            FilterOp::Equals => {
                parts.push(format!("{col}::text = ${param_idx}"));
                values.push(f.value.clone());
            }
            FilterOp::NotEquals => {
                parts.push(format!("{col}::text != ${param_idx}"));
                values.push(f.value.clone());
            }
            FilterOp::GreaterThan => {
                parts.push(format!("{col}::text > ${param_idx}"));
                values.push(f.value.clone());
            }
            FilterOp::LessThan => {
                parts.push(format!("{col}::text < ${param_idx}"));
                values.push(f.value.clone());
            }
        }
    }

    let clause = format!(" WHERE {}", parts.join(" AND "));
    (clause, values)
}

fn filter_values_as_refs(values: &[String]) -> Vec<&(dyn tokio_postgres::types::ToSql + Sync)> {
    values
        .iter()
        .map(|v| v as &(dyn tokio_postgres::types::ToSql + Sync))
        .collect()
}

/// Try to extract a typed value, falling back gracefully on error.
macro_rules! try_col {
    ($row:expr, $idx:expr, $T:ty) => {
        match $row.try_get::<_, Option<$T>>($idx) {
            Ok(Some(v)) => v.to_string(),
            Ok(None) => "NULL".to_string(),
            Err(_) => try_as_string($row, $idx),
        }
    };
    ($row:expr, $idx:expr, $T:ty, $fmt:expr) => {
        match $row.try_get::<_, Option<$T>>($idx) {
            Ok(Some(v)) => $fmt(v),
            Ok(None) => "NULL".to_string(),
            Err(_) => try_as_string($row, $idx),
        }
    };
}

/// Last-resort: try as String, then give up with a placeholder.
fn try_as_string(row: &Row, idx: usize) -> String {
    row.try_get::<_, Option<String>>(idx)
        .ok()
        .flatten()
        .unwrap_or_else(|| "?".to_string())
}

/// Convert a PostgreSQL row to a vector of display strings.
fn row_to_strings(row: &Row) -> Vec<String> {
    let columns = row.columns();
    let mut values = Vec::with_capacity(columns.len());

    for (i, col) in columns.iter().enumerate() {
        let value = match col.type_().name() {
            "bool" => try_col!(row, i, bool),
            "int2" => try_col!(row, i, i16),
            "int4" => try_col!(row, i, i32),
            "int8" => try_col!(row, i, i64),
            "float4" => try_col!(row, i, f32),
            "float8" => try_col!(row, i, f64),
            "numeric" => {
                // numeric can exceed f64 precision; try String first
                try_as_string(row, i)
            }
            "json" | "jsonb" => try_col!(row, i, serde_json::Value),
            "timestamptz" => {
                try_col!(row, i, chrono::DateTime<chrono::Utc>, |v: chrono::DateTime<chrono::Utc>| v
                    .to_rfc3339())
            }
            "timestamp" => {
                try_col!(row, i, chrono::NaiveDateTime, |v: chrono::NaiveDateTime| v
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string())
            }
            "date" => try_col!(row, i, chrono::NaiveDate),
            _ => try_as_string(row, i),
        };
        values.push(value);
    }

    values
}
