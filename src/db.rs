//! SQLite schema and inserts.
//!
//! Two sources feed the database:
//! * the search list (`products` and its child tables), one row per cpId;
//! * the detail record (`product_details`, `components`, `classifications`,
//!   `h_phrases`, `p_phrases`, `symbols`, `trade_names`).
//!
//! Every row also keeps the untouched JSON (`raw_json`) so nothing is lost
//! if the normalised columns turn out to be incomplete.

use rusqlite::{params, Connection, Transaction};
use serde_json::Value;

pub type Error = Box<dyn std::error::Error + Send + Sync>;

pub fn open(path: &str) -> Result<Connection, Error> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;",
    )?;
    create_schema(&conn)?;
    Ok(conn)
}

fn create_schema(conn: &Connection) -> Result<(), Error> {
    conn.execute_batch(
        r#"
CREATE TABLE IF NOT EXISTS products (
    cp_id                       TEXT PRIMARY KEY,
    id                          INTEGER,
    product_version_id          INTEGER,
    primary_name                TEXT,
    type_code                   TEXT,
    type_de                     TEXT,
    type_fr                     TEXT,
    type_it                     TEXT,
    type_en                     TEXT,
    status_code                 TEXT,
    status_de                   TEXT,
    status_indicator            TEXT,
    licence_holder_id           INTEGER,
    licence_holder_cid          TEXT,
    licence_holder_name         TEXT,
    licence_holder_uid          TEXT,
    licence_holder_street       TEXT,
    licence_holder_house_number TEXT,
    licence_holder_zip          TEXT,
    licence_holder_city         TEXT,
    licence_holder_canton       TEXT,
    licence_holder_country      TEXT,
    licence_holder_foreign      INTEGER,
    last_change                 TEXT,
    deleted                     TEXT,
    removed_from_market         TEXT,
    fertilizer_expiration_date  TEXT,
    has_components              INTEGER,
    approval_number             TEXT,
    approval_valid_to           TEXT,
    user_categories             TEXT,
    state_of_matter_code        TEXT,
    state_of_matter_de          TEXT,
    trade_names                 TEXT,
    ufis                        TEXT,
    ghs_h_sentences             TEXT,
    ghs_symbols                 TEXT,
    product_usages              TEXT,
    notification_date           TEXT,
    last_status_change          TEXT,
    is_type_fertilizer          INTEGER,
    is_type_biocide             INTEGER,
    raw_json                    TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS product_details (
    cp_id                   TEXT PRIMARY KEY REFERENCES products(cp_id),
    fetched_at              TEXT NOT NULL,
    modified                TEXT,
    last_change             TEXT,
    parent_product_id       INTEGER,
    formulation_type_code   TEXT,
    formulation_remark      TEXT,
    ufis                    TEXT,
    no_classification       INTEGER,
    classified              INTEGER,
    classification_number   TEXT,
    no_labeling             INTEGER,
    signalword              TEXT,
    for_commercial_use      INTEGER,
    for_consumer_use        INTEGER,
    usages                  TEXT,
    state_of_matter_code    TEXT,
    other_state_of_matter   TEXT,
    substance_type          TEXT,
    approval_type           TEXT,
    approval_number         TEXT,
    approval_valid_to       TEXT,
    valid_from              TEXT,
    valid_to                TEXT,
    fertilizer_type         TEXT,
    fertilizer_type_name    TEXT,
    biocide_subtypes        TEXT,
    biocide_efficacies      TEXT,
    biocide_app_areas       TEXT,
    biocide_app_methods     TEXT,
    biocide_app_targets     TEXT,
    external_remarks        TEXT,
    raw_json                TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS components (
    cp_id                   TEXT NOT NULL REFERENCES products(cp_id),
    component_id            INTEGER,
    formatted_id            TEXT,
    parameter_id            TEXT,
    parameter_name          TEXT,
    casrn                   TEXT,
    ec_numbers              TEXT,
    concentration_min       REAL,
    concentration_max       REAL,
    concentration_text      TEXT,
    displayed_unit          TEXT,
    fnc                     TEXT,
    name                    TEXT,
    declared                INTEGER,
    declared_sdb            INTEGER,
    minimal_concentration   INTEGER NOT NULL DEFAULT 0,
    raw_json                TEXT NOT NULL,
    PRIMARY KEY (cp_id, component_id, minimal_concentration)
);

CREATE TABLE IF NOT EXISTS classifications (
    cp_id           TEXT NOT NULL REFERENCES products(cp_id),
    category_name   TEXT,
    class_name      TEXT,
    category_de     TEXT,
    category_fr     TEXT,
    category_it     TEXT,
    category_en     TEXT,
    class_de        TEXT,
    class_fr        TEXT,
    class_it        TEXT,
    class_en        TEXT,
    h_phrases       TEXT,
    PRIMARY KEY (cp_id, category_name)
);

CREATE TABLE IF NOT EXISTS h_phrases (
    cp_id   TEXT NOT NULL REFERENCES products(cp_id),
    code    TEXT NOT NULL,
    text_de TEXT, text_fr TEXT, text_it TEXT, text_en TEXT,
    PRIMARY KEY (cp_id, code)
);

CREATE TABLE IF NOT EXISTS p_phrases (
    cp_id   TEXT NOT NULL REFERENCES products(cp_id),
    code    TEXT NOT NULL,
    text_de TEXT, text_fr TEXT, text_it TEXT, text_en TEXT,
    PRIMARY KEY (cp_id, code)
);

CREATE TABLE IF NOT EXISTS symbols (
    cp_id   TEXT NOT NULL REFERENCES products(cp_id),
    code    TEXT NOT NULL,
    text_de TEXT, text_fr TEXT, text_it TEXT, text_en TEXT,
    PRIMARY KEY (cp_id, code)
);

CREATE TABLE IF NOT EXISTS trade_names (
    cp_id   TEXT NOT NULL REFERENCES products(cp_id),
    name    TEXT NOT NULL,
    PRIMARY KEY (cp_id, name)
);

CREATE INDEX IF NOT EXISTS idx_products_name        ON products(primary_name);
CREATE INDEX IF NOT EXISTS idx_products_holder_cid  ON products(licence_holder_cid);
CREATE INDEX IF NOT EXISTS idx_products_type        ON products(type_code);
CREATE INDEX IF NOT EXISTS idx_components_casrn     ON components(casrn);
CREATE INDEX IF NOT EXISTS idx_components_param     ON components(parameter_id);
"#,
    )?;
    Ok(())
}

// ---------------------------------------------------------------- helpers

fn s(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(x) => Some(x.clone()),
        other => Some(other.to_string()),
    }
}
fn get<'a>(v: &'a Value, path: &[&str]) -> &'a Value {
    let mut cur = v;
    for p in path {
        cur = cur.get(p).unwrap_or(&Value::Null);
    }
    cur
}
fn gs(v: &Value, path: &[&str]) -> Option<String> {
    s(get(v, path))
}
fn gi(v: &Value, path: &[&str]) -> Option<i64> {
    get(v, path).as_i64()
}
fn gf(v: &Value, path: &[&str]) -> Option<f64> {
    get(v, path).as_f64()
}
fn gb(v: &Value, path: &[&str]) -> Option<bool> {
    get(v, path).as_bool()
}
/// Join `arr[*][key]` (or the bare strings of `arr`) with `; `.
fn join(v: &Value, key: Option<&str>) -> Option<String> {
    let arr = v.as_array()?;
    let parts: Vec<String> = arr
        .iter()
        .filter_map(|e| match key {
            Some(k) => s(e.get(k)?),
            None => s(e),
        })
        .collect();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("; "))
    }
}

// ---------------------------------------------------------------- products

pub fn insert_product(tx: &Transaction, p: &Value) -> Result<(), Error> {
    let cp_id = gs(p, &["cpId"]).ok_or("product without cpId")?;
    tx.execute(
        r#"INSERT OR REPLACE INTO products (
            cp_id, id, product_version_id, primary_name,
            type_code, type_de, type_fr, type_it, type_en,
            status_code, status_de, status_indicator,
            licence_holder_id, licence_holder_cid, licence_holder_name, licence_holder_uid,
            licence_holder_street, licence_holder_house_number, licence_holder_zip,
            licence_holder_city, licence_holder_canton, licence_holder_country, licence_holder_foreign,
            last_change, deleted, removed_from_market, fertilizer_expiration_date,
            has_components, approval_number, approval_valid_to, user_categories,
            state_of_matter_code, state_of_matter_de, trade_names, ufis,
            ghs_h_sentences, ghs_symbols, product_usages,
            notification_date, last_status_change, is_type_fertilizer, is_type_biocide, raw_json
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,
                  ?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37,?38,?39,?40,?41,?42,?43)"#,
        params![
            cp_id,
            gi(p, &["id"]),
            gi(p, &["productVersionId"]),
            gs(p, &["primaryName"]),
            gs(p, &["type", "code"]),
            gs(p, &["type", "textDe"]),
            gs(p, &["type", "textFr"]),
            gs(p, &["type", "textIt"]),
            gs(p, &["type", "textEn"]),
            gs(p, &["productStatus", "code"]),
            gs(p, &["productStatus", "textDe"]),
            gs(p, &["productStatusIndicator"]),
            gi(p, &["licenceHolder", "licenceHolderId"]),
            gs(p, &["licenceHolder", "cid"]),
            gs(p, &["licenceHolder", "name"]),
            gs(p, &["licenceHolder", "puid", "uidAsText"]),
            gs(p, &["licenceHolder", "street"]),
            gs(p, &["licenceHolder", "houseNumber"]),
            gs(p, &["licenceHolder", "zip"]),
            gs(p, &["licenceHolder", "city"]),
            gs(p, &["licenceHolder", "canton", "code"]),
            gs(p, &["licenceHolder", "country", "code"]),
            gb(p, &["licenceHolder", "foreignOrg"]),
            gs(p, &["lastChange"]),
            gs(p, &["deleted"]),
            gs(p, &["removedFromMarket"]),
            gs(p, &["fertilizerExpirationDate"]),
            gb(p, &["hasComponents"]),
            gs(p, &["approvalNumber"]),
            gs(p, &["approvalValidTo"]),
            gs(p, &["userCategories"]),
            gs(p, &["stateOfMatterCode", "code"]),
            gs(p, &["stateOfMatterCode", "textDe"]),
            join(get(p, &["tradeNames"]), None),
            join(get(p, &["ufis"]), None),
            join(get(p, &["ghsHSentences"]), Some("name")),
            join(get(p, &["ghsSymbols"]), Some("code")),
            join(get(p, &["productUsages"]), Some("code")),
            gs(p, &["notificationDate"]),
            gs(p, &["lastStatusChange"]),
            gb(p, &["isTypeFertilizer"]),
            gb(p, &["isTypeBiocide"]),
            p.to_string(),
        ],
    )?;

    // Trade names from the list (the detail record has `nameList` too).
    tx.execute("DELETE FROM trade_names WHERE cp_id = ?1", params![cp_id])?;
    if let Some(names) = get(p, &["tradeNames"]).as_array() {
        for n in names.iter().filter_map(Value::as_str) {
            tx.execute(
                "INSERT OR IGNORE INTO trade_names (cp_id, name) VALUES (?1, ?2)",
                params![cp_id, n],
            )?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- details

fn insert_phrases(tx: &Transaction, table: &str, cp_id: &str, arr: &Value) -> Result<(), Error> {
    let Some(arr) = arr.as_array() else { return Ok(()) };
    let sql = format!(
        "INSERT OR IGNORE INTO {} (cp_id, code, text_de, text_fr, text_it, text_en) VALUES (?1,?2,?3,?4,?5,?6)",
        table
    );
    for ph in arr {
        let code = match gs(ph, &["name"]).or_else(|| gs(ph, &["code"])) {
            Some(c) => c,
            None => continue,
        };
        // phrases carry `textMap.{de,fr,it,en}`, code lists carry `textDe`...
        let (de, fr, it, en) = if ph.get("textMap").is_some() {
            (
                gs(ph, &["textMap", "de"]),
                gs(ph, &["textMap", "fr"]),
                gs(ph, &["textMap", "it"]),
                gs(ph, &["textMap", "en"]),
            )
        } else {
            (
                gs(ph, &["textDe"]),
                gs(ph, &["textFr"]),
                gs(ph, &["textIt"]),
                gs(ph, &["textEn"]),
            )
        };
        tx.execute(&sql, params![cp_id, code, de, fr, it, en])?;
    }
    Ok(())
}

fn insert_components(tx: &Transaction, cp_id: &str, arr: &Value, minimal: bool) -> Result<(), Error> {
    let Some(arr) = arr.as_array() else { return Ok(()) };
    for c in arr {
        tx.execute(
            r#"INSERT OR REPLACE INTO components (
                cp_id, component_id, formatted_id, parameter_id, parameter_name, casrn, ec_numbers,
                concentration_min, concentration_max, concentration_text, displayed_unit,
                fnc, name, declared, declared_sdb, minimal_concentration, raw_json
            ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)"#,
            params![
                cp_id,
                gi(c, &["id"]),
                gs(c, &["formattedId"]),
                gs(c, &["parameter", "parameterId"]),
                gs(c, &["parameter", "primaryName"]),
                gs(c, &["parameter", "casrn"]),
                gs(c, &["parameter", "ecNumbers"]),
                gf(c, &["concentrationMin"]),
                gf(c, &["concentrationMax"]),
                gs(c, &["concentrationToString"]),
                gs(c, &["displayedUnit"]),
                gs(c, &["fnc"]),
                gs(c, &["name"]),
                gb(c, &["declared"]),
                gb(c, &["declaredSDB"]),
                minimal as i64,
                c.to_string(),
            ],
        )?;
    }
    Ok(())
}

pub fn insert_detail(tx: &Transaction, cp_id: &str, d: &Value, fetched_at: &str) -> Result<(), Error> {
    tx.execute(
        r#"INSERT OR REPLACE INTO product_details (
            cp_id, fetched_at, modified, last_change, parent_product_id,
            formulation_type_code, formulation_remark, ufis,
            no_classification, classified, classification_number,
            no_labeling, signalword, for_commercial_use, for_consumer_use, usages,
            state_of_matter_code, other_state_of_matter, substance_type,
            approval_type, approval_number, approval_valid_to, valid_from, valid_to,
            fertilizer_type, fertilizer_type_name,
            biocide_subtypes, biocide_efficacies, biocide_app_areas, biocide_app_methods, biocide_app_targets,
            external_remarks, raw_json
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,
                  ?24,?25,?26,?27,?28,?29,?30,?31,?32,?33)"#,
        params![
            cp_id,
            fetched_at,
            gs(d, &["modified"]),
            gs(d, &["lastChange"]),
            gi(d, &["parentProductId"]),
            gs(d, &["formulation", "formulationTypeCode"]),
            gs(d, &["formulation", "remark"]),
            join(get(d, &["formulation", "ufis"]), None),
            gb(d, &["classification", "noClassification"]),
            gb(d, &["classification", "classified"]),
            gs(d, &["classificationNumber"]),
            gb(d, &["labeling", "noLabeling"]),
            gs(d, &["labeling", "signalword", "code"]).or_else(|| gs(d, &["labeling", "signalword"])),
            gb(d, &["forCommercialUse"]),
            gb(d, &["forConsumerUse"]),
            join(get(d, &["usageList"]), Some("code")),
            gs(d, &["stateOfMatter", "code"]),
            gs(d, &["otherStateOfMatter"]),
            gs(d, &["substanceType", "code"]).or_else(|| gs(d, &["substanceType"])),
            gs(d, &["approvalType", "code"]).or_else(|| gs(d, &["approvalType"])),
            gs(d, &["approvalNumber"]),
            gs(d, &["approvalValidTo"]),
            gs(d, &["validFrom"]),
            gs(d, &["validTo"]),
            gs(d, &["fertilizerType", "code"]).or_else(|| gs(d, &["fertilizerType"])),
            gs(d, &["fertilizerTypeName"]),
            join(get(d, &["biocideSubtypeList"]), Some("code")),
            join(get(d, &["biocideEfficacyList"]), Some("code")),
            join(get(d, &["biocideAppAreaList"]), Some("code")),
            join(get(d, &["biocideAppMethodList"]), Some("code")),
            join(get(d, &["biocideAppTargetList"]), Some("code")),
            gs(d, &["externalRemarks"]),
            d.to_string(),
        ],
    )?;

    for t in ["components", "classifications", "h_phrases", "p_phrases", "symbols"] {
        tx.execute(&format!("DELETE FROM {} WHERE cp_id = ?1", t), params![cp_id])?;
    }

    insert_components(tx, cp_id, get(d, &["formulation", "components"]), false)?;
    insert_components(tx, cp_id, get(d, &["formulation", "minimalConcentrationComponents"]), true)?;

    if let Some(cats) = get(d, &["classification", "classificationCategories"]).as_array() {
        for c in cats {
            tx.execute(
                r#"INSERT OR IGNORE INTO classifications (
                    cp_id, category_name, class_name,
                    category_de, category_fr, category_it, category_en,
                    class_de, class_fr, class_it, class_en, h_phrases
                ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"#,
                params![
                    cp_id,
                    gs(c, &["name"]),
                    gs(c, &["className"]),
                    gs(c, &["categoryTextMap", "de"]),
                    gs(c, &["categoryTextMap", "fr"]),
                    gs(c, &["categoryTextMap", "it"]),
                    gs(c, &["categoryTextMap", "en"]),
                    gs(c, &["classTextMap", "de"]),
                    gs(c, &["classTextMap", "fr"]),
                    gs(c, &["classTextMap", "it"]),
                    gs(c, &["classTextMap", "en"]),
                    gs(c, &["hPhrasesText"]).or_else(|| join(get(c, &["hPhrases"]), Some("name"))),
                ],
            )?;
        }
    }

    insert_phrases(tx, "h_phrases", cp_id, get(d, &["labeling", "hPhrases"]))?;
    insert_phrases(tx, "p_phrases", cp_id, get(d, &["labeling", "pPhrases"]))?;
    insert_phrases(tx, "symbols", cp_id, get(d, &["labeling", "symbols"]))?;

    if let Some(names) = get(d, &["nameList"]).as_array() {
        for n in names {
            let name = n.as_str().map(str::to_string).or_else(|| gs(n, &["name"]));
            if let Some(name) = name {
                tx.execute(
                    "INSERT OR IGNORE INTO trade_names (cp_id, name) VALUES (?1, ?2)",
                    params![cp_id, name],
                )?;
            }
        }
    }
    Ok(())
}

/// cpIds present in `products` but not yet in `product_details`.
pub fn missing_details(conn: &Connection) -> Result<Vec<String>, Error> {
    let mut stmt = conn.prepare(
        "SELECT p.cp_id FROM products p
         LEFT JOIN product_details d ON d.cp_id = p.cp_id
         WHERE d.cp_id IS NULL ORDER BY p.cp_id",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

pub fn count(conn: &Connection, table: &str) -> Result<i64, Error> {
    Ok(conn.query_row(&format!("SELECT COUNT(*) FROM {}", table), [], |r| r.get(0))?)
}
