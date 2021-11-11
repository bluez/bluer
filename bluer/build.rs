use serde::Deserialize;
use std::{collections::HashMap, env, error::Error, fmt, fs::File, io::Write, path::Path, str::FromStr};
use uuid::Uuid;

#[path = "src/uuid_ext.rs"]
mod uuid_ext;
use uuid_ext::UuidExt;

struct UuidOrShort(Uuid);

impl FromStr for UuidOrShort {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.parse::<Uuid>() {
            Ok(uuid) => Ok(Self(uuid)),
            Err(_) => match u16::from_str_radix(s, 16) {
                Ok(short) => Ok(Self(Uuid::from_u16(short))),
                Err(_) => match u32::from_str_radix(s, 16) {
                    Ok(short) => Ok(Self(Uuid::from_u32(short))),
                    Err(_) => Err(s.to_string()),
                },
            },
        }
    }
}

impl fmt::Display for UuidOrShort {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Uuid::from_u128({})", self.0.as_u128())
    }
}

impl UuidOrShort {
    fn as_u128(&self) -> u128 {
        self.0.as_u128()
    }
}

#[derive(Deserialize)]
struct UuidEntry {
    name: String,
    identifier: String,
    uuid: String,
    #[serde(default)]
    source: String,
}

impl UuidEntry {
    fn rust_id(&self, prefix: &str) -> String {
        let id = self.identifier.trim_start_matches(prefix);
        let mut rid = String::new();
        let mut capital = true;
        for c in id.trim().chars() {
            if !c.is_alphanumeric() {
                capital = true;
            } else if capital {
                rid.push(c.to_ascii_uppercase());
                capital = false;
            } else {
                rid.push(c);
            }
        }
        rid
    }

    fn uuid(&self) -> Result<UuidOrShort, String> {
        self.uuid.parse::<UuidOrShort>()
    }
}

fn convert_uuids(src: &str, dest: &str, name: &str, doc_name: &str, prefix: &str) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={}", src);

    let input = File::open(src)?;
    let entries: Vec<UuidEntry> = serde_json::from_reader(input)?;
    let mut out = File::create(Path::new(&env::var("OUT_DIR")?).join(dest))?;

    writeln!(out, "/// Assigned identifiers for {}.", doc_name)?;
    writeln!(out, "///")?;
    writeln!(out, "/// Can be converted to and from UUIDs.")?;
    writeln!(out, "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString, Display)]")?;
    writeln!(out, "#[non_exhaustive]")?;
    writeln!(out, "pub enum {} {{", name)?;
    for entry in &entries {
        writeln!(out, "    /// {}", &entry.name)?;
        if !entry.source.is_empty() {
            writeln!(out, "    ///")?;
            writeln!(out, "    /// Source: {}", &entry.source)?;
        }
        writeln!(out, "    #[strum(serialize = \"{}\")]", &entry.name)?;
        writeln!(out, "    {},", entry.rust_id(prefix))?;
    }
    writeln!(out, "}}")?;

    writeln!(out, "impl From<{}> for Uuid {{", name)?;
    writeln!(out, "    fn from(s: {}) -> Uuid {{", name)?;
    writeln!(out, "        match s {{")?;
    for entry in &entries {
        writeln!(out, "            {}::{} => {},", name, entry.rust_id(prefix), entry.uuid()?)?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    writeln!(out, "impl TryFrom<Uuid> for {} {{", name)?;
    writeln!(out, "    type Error = Uuid;")?;
    writeln!(out, "    fn try_from(uuid: Uuid) -> Result<Self, Uuid> {{")?;
    writeln!(out, "        #[allow(unreachable_patterns)]")?;
    writeln!(out, "        match uuid.as_u128() {{")?;
    for entry in entries {
        writeln!(out, "            {} => Ok(Self::{}),", entry.uuid()?.as_u128(), entry.rust_id(prefix))?;
    }
    writeln!(out, "            _ => Err(uuid),")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    Ok(())
}

#[derive(Deserialize)]
struct CodeEntry {
    code: u16,
    name: String,
}

impl CodeEntry {
    fn rust_id(&self) -> String {
        let mut rid = String::new();
        let mut capital = true;
        let mut first = true;
        for c in self.name.trim().chars() {
            if first && !c.is_alphabetic() {
                rid.push('N');
            }
            first = false;

            if !c.is_ascii_alphanumeric() {
                capital = true;
            } else if capital {
                rid.push(c.to_ascii_uppercase());
                capital = false;
            } else {
                rid.push(c);
            }
        }
        rid
    }
}

fn convert_ids(src: &str, dest: &str, name: &str, doc_name: &str) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={}", src);

    let input = File::open(src)?;
    let mut entries: Vec<CodeEntry> = serde_json::from_reader(input)?;
    let mut out = File::create(Path::new(&env::var("OUT_DIR")?).join(dest))?;

    let mut seen_names: HashMap<String, usize> = HashMap::new();
    for entry in &mut entries {
        let s = seen_names.entry(entry.rust_id()).or_default();
        if *s > 0 {
            entry.name = format!("{} ({})", &entry.name, s);
        }
        *s += 1;
    }

    writeln!(out, "/// Assigned identifiers for {}.", doc_name)?;
    writeln!(out, "///")?;
    writeln!(out, "/// Can be converted to and from ids.")?;
    writeln!(out, "#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, EnumString, Display)]")?;
    writeln!(out, "#[non_exhaustive]")?;
    writeln!(out, "pub enum {} {{", name)?;
    for service in &entries {
        writeln!(out, "    /// {}", &service.name)?;
        writeln!(out, "    #[strum(serialize = \"{}\")]", &service.name.replace('"', "\\\""))?;
        writeln!(out, "    {},", service.rust_id())?;
    }
    writeln!(out, "}}")?;

    writeln!(out, "impl From<{}> for u16 {{", name)?;
    writeln!(out, "    fn from(s: {}) -> u16 {{", name)?;
    writeln!(out, "        match s {{")?;
    for entry in &entries {
        writeln!(out, "            {}::{} => {},", name, entry.rust_id(), entry.code)?;
    }
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    writeln!(out, "impl TryFrom<u16> for {} {{", name)?;
    writeln!(out, "    type Error = u16;")?;
    writeln!(out, "    fn try_from(code: u16) -> Result<Self, u16> {{")?;
    writeln!(out, "        #[allow(unreachable_patterns)]")?;
    writeln!(out, "        match code {{")?;
    for entry in entries {
        writeln!(out, "            {} => Ok(Self::{}),", entry.code, entry.rust_id())?;
    }
    writeln!(out, "            _ => Err(code),")?;
    writeln!(out, "        }}")?;
    writeln!(out, "    }}")?;
    writeln!(out, "}}")?;
    writeln!(out)?;

    Ok(())
}

fn build_ids() -> Result<(), Box<dyn Error>> {
    convert_uuids(
        "service_class_uuids.json",
        "service_class.inc",
        "ServiceClass",
        "service classes and profiles",
        " ",
    )
    .expect("service classes");

    convert_uuids(
        "bluetooth-numbers-database/v1/service_uuids.json",
        "service.inc",
        "Service",
        "GATT services",
        "org.bluetooth.service.",
    )
    .expect("services");

    convert_uuids(
        "bluetooth-numbers-database/v1/characteristic_uuids.json",
        "characteristic.inc",
        "Characteristic",
        "GATT characteristics",
        "org.bluetooth.characteristic.",
    )
    .expect("characteristics");

    convert_uuids(
        "bluetooth-numbers-database/v1/descriptor_uuids.json",
        "descriptor.inc",
        "Descriptor",
        "GATT descriptors",
        "org.bluetooth.descriptor.",
    )
    .expect("descriptors");

    convert_ids("bluetooth-numbers-database/v1/company_ids.json", "company.inc", "Manufacturer", "manufacturers")
        .expect("companys");

    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    if env::var_os("CARGO_FEATURE_ID").is_some() {
        build_ids()?;
    }

    Ok(())
}
