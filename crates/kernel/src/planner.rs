use std::fmt;
use std::fs;
use std::io;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::de::{MapAccess, Visitor, value::MapAccessDeserializer};
use serde::{Deserialize, Deserializer, Serialize};

const MAX_JSON_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Task {
    pub schema_version: u32,
    pub kind: String,
    pub task_id: String,
    pub revision: u64,
    pub name: String,
    pub engine_executable: String,
    pub source: LocalPath,
    pub destination: LocalPath,
    pub options: Vec<TaskOption>,
}

// A derived struct deserializer also accepts positional JSON arrays. These input
// DTOs require objects; MapAccessDeserializer retains duplicate-field detection.
fn object_only<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct ObjectVisitor<T>(PhantomData<T>);

    impl<'de, T: Deserialize<'de>> Visitor<'de> for ObjectVisitor<T> {
        type Value = T;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("an object")
        }

        fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<T, M::Error> {
            T::deserialize(MapAccessDeserializer::new(map))
        }
    }

    deserializer.deserialize_map(ObjectVisitor(PhantomData))
}

impl<'de> Deserialize<'de> for Task {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Fields {
            schema_version: u32,
            kind: String,
            task_id: String,
            revision: u64,
            name: String,
            engine_executable: String,
            source: LocalPath,
            destination: LocalPath,
            options: Vec<TaskOption>,
        }

        let fields: Fields = object_only(deserializer)?;
        Ok(Self {
            schema_version: fields.schema_version,
            kind: fields.kind,
            task_id: fields.task_id,
            revision: fields.revision,
            name: fields.name,
            engine_executable: fields.engine_executable,
            source: fields.source,
            destination: fields.destination,
            options: fields.options,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalPath {
    pub raw: String,
}

impl<'de> Deserialize<'de> for LocalPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            raw: String,
        }

        let fields: Fields = object_only(deserializer)?;
        Ok(Self { raw: fields.raw })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskOption {
    Recursive,
    Archive,
}

// Deserialize the object directly: buffering it in a map would lose duplicate keys.
impl<'de> Deserialize<'de> for TaskOption {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct OptionObject {
            kind: String,
        }

        let object: OptionObject = object_only(deserializer)?;
        match object.kind.as_str() {
            "recursive" => Ok(Self::Recursive),
            "archive" => Ok(Self::Archive),
            other => Err(serde::de::Error::unknown_variant(
                other,
                &["recursive", "archive"],
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathCheck {
    CheckedAtPlanning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineCheck {
    NotPerformed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InvocationPlan {
    pub contract_version: String,
    pub task_snapshot: Task,
    pub executable: String,
    pub run_argv: Vec<String>,
    pub dry_run_argv: Vec<String>,
    pub path_check: PathCheck,
    pub engine_check: EngineCheck,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningErrorCode {
    InvalidJson,
    UnsupportedSchema,
    InvalidTask,
    InvalidPath,
    UnsafePathRelation,
    PathAccessDenied,
    PathIo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningError {
    pub code: PlanningErrorCode,
    pub field: Option<String>,
    pub message: String,
    pub os_code: Option<i32>,
}

impl fmt::Display for PlanningError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(formatter, "{field}: {}", self.message),
            None => formatter.write_str(&self.message),
        }
    }
}

impl std::error::Error for PlanningError {}

fn error(code: PlanningErrorCode, field: &str, message: &str) -> PlanningError {
    PlanningError {
        code,
        field: Some(field.into()),
        message: message.into(),
        os_code: None,
    }
}

/// Parse strict planner.v1 task JSON and validate its values without filesystem I/O.
pub fn parse_task_json(json: &str) -> Result<Task, PlanningError> {
    if json.len() > MAX_JSON_BYTES {
        return Err(error(
            PlanningErrorCode::InvalidTask,
            "json",
            "task JSON exceeds 64 KiB",
        ));
    }
    let task = serde_json::from_str(json).map_err(|failure: serde_json::Error| PlanningError {
        code: if failure.is_syntax() || failure.is_eof() {
            PlanningErrorCode::InvalidJson
        } else {
            PlanningErrorCode::InvalidTask
        },
        field: None,
        message: failure.to_string(),
        os_code: None,
    })?;
    validate_task(&task)?;
    Ok(task)
}

/// Validate task values and raw path expressions; does not inspect the filesystem.
pub fn validate_task(task: &Task) -> Result<(), PlanningError> {
    if task.schema_version != 1 {
        return Err(error(
            PlanningErrorCode::UnsupportedSchema,
            "schemaVersion",
            "only schemaVersion 1 is supported",
        ));
    }
    if task.kind != "rsync.task.pot" {
        return Err(error(
            PlanningErrorCode::InvalidTask,
            "kind",
            "expected rsync.task.pot",
        ));
    }
    for (field, value, maximum) in [
        ("taskId", task.task_id.as_str(), 128),
        ("name", task.name.as_str(), 256),
    ] {
        if value.is_empty()
            || value.len() > maximum
            || value.trim().is_empty()
            || value.contains('\0')
        {
            return Err(error(
                PlanningErrorCode::InvalidTask,
                field,
                "text must be nonblank, within its UTF-8 byte limit and contain no NUL",
            ));
        }
    }
    if task.revision == 0 {
        return Err(error(
            PlanningErrorCode::InvalidTask,
            "revision",
            "revision must be at least 1",
        ));
    }
    if task.options.len() != 1 {
        return Err(error(
            PlanningErrorCode::InvalidTask,
            "options",
            "exactly one recursive or archive option is required",
        ));
    }
    for (field, raw) in [
        ("engineExecutable", task.engine_executable.as_str()),
        ("source.raw", task.source.raw.as_str()),
        ("destination.raw", task.destination.raw.as_str()),
    ] {
        if raw.is_empty() || raw.len() > 4096 || raw.contains('\0') || !Path::new(raw).is_absolute()
        {
            return Err(error(
                PlanningErrorCode::InvalidPath,
                field,
                "path must be absolute, within 1..4096 UTF-8 bytes and contain no NUL",
            ));
        }
    }
    Ok(())
}

struct Directory {
    canonical: PathBuf,
    #[cfg(unix)]
    identity: (u64, u64),
}

fn path_error(failure: io::Error, field: &str) -> PlanningError {
    let code = match failure.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => PlanningErrorCode::InvalidPath,
        io::ErrorKind::PermissionDenied => PlanningErrorCode::PathAccessDenied,
        _ => PlanningErrorCode::PathIo,
    };
    PlanningError {
        code,
        field: Some(field.into()),
        message: failure.to_string(),
        os_code: failure.raw_os_error(),
    }
}

fn inspect_directory(raw: &str, field: &str) -> Result<Directory, PlanningError> {
    let metadata = fs::metadata(raw).map_err(|failure| path_error(failure, field))?;
    if !metadata.is_dir() {
        return Err(error(
            PlanningErrorCode::InvalidPath,
            field,
            "path must name an existing directory",
        ));
    }
    let canonical = fs::canonicalize(raw).map_err(|failure| path_error(failure, field))?;
    if canonical.parent().is_none() {
        return Err(error(
            PlanningErrorCode::InvalidPath,
            field,
            "filesystem root is outside the local planning boundary",
        ));
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        (metadata.dev(), metadata.ino())
    };
    Ok(Directory {
        canonical,
        #[cfg(unix)]
        identity,
    })
}

fn same_directory(source: &Directory, destination: &Directory) -> bool {
    if source.canonical == destination.canonical {
        return true;
    }
    #[cfg(unix)]
    if source.identity == destination.identity {
        return true;
    }
    false
}

/// Check existing local directories and create argv; never inspect or run the engine.
/// Checks describe the filesystem at planning time and do not authorize execution.
pub fn build_local_plan(task: Task) -> Result<InvocationPlan, PlanningError> {
    validate_task(&task)?;
    let source = inspect_directory(&task.source.raw, "source.raw")?;
    let destination = inspect_directory(&task.destination.raw, "destination.raw")?;
    let relation = if same_directory(&source, &destination) {
        Some("source and destination name the same directory")
    } else if destination.canonical.starts_with(&source.canonical) {
        Some("destination is nested inside source")
    } else if source.canonical.starts_with(&destination.canonical) {
        Some("source is nested inside destination")
    } else {
        None
    };
    if let Some(message) = relation {
        return Err(error(
            PlanningErrorCode::UnsafePathRelation,
            "destination.raw",
            message,
        ));
    }
    let selected_option = match &task.options[0] {
        TaskOption::Recursive => "--recursive",
        TaskOption::Archive => "--archive",
    };
    let run_argv = vec![
        selected_option.into(),
        "--".into(),
        task.source.raw.clone(),
        task.destination.raw.clone(),
    ];
    let dry_run_argv = vec![
        selected_option.into(),
        "--dry-run".into(),
        "--".into(),
        task.source.raw.clone(),
        task.destination.raw.clone(),
    ];
    Ok(InvocationPlan {
        contract_version: "planner.v1".into(),
        executable: task.engine_executable.clone(),
        task_snapshot: task,
        run_argv,
        dry_run_argv,
        path_check: PathCheck::CheckedAtPlanning,
        engine_check: EngineCheck::NotPerformed,
    })
}
