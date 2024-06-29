//! Bytecode related types.

use crate::{
    serde_helpers,
    sourcemap::{self, SourceMap, SyntaxError},
    FunctionDebugData, GeneratedSource, Offsets,
};
use alloy_primitives::{hex, Address, Bytes};
use foundry_compilers_core::utils;
use serde::{Deserialize, Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bytecode {
    /// Debugging information at function level
    #[serde(default, skip_serializing_if = "::std::collections::BTreeMap::is_empty")]
    pub function_debug_data: BTreeMap<String, FunctionDebugData>,
    /// The bytecode as a hex string.
    #[serde(serialize_with = "serialize_bytecode_without_prefix")]
    pub object: BytecodeObject,
    /// Opcodes list (string)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opcodes: Option<String>,
    /// The source mapping as a string. See the source mapping definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map: Option<String>,
    /// Array of sources generated by the compiler. Currently only contains a
    /// single Yul file.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_sources: Vec<GeneratedSource>,
    /// If given, this is an unlinked object.
    #[serde(default)]
    pub link_references: BTreeMap<String, BTreeMap<String, Vec<Offsets>>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactBytecode {
    /// The bytecode as a hex string.
    pub object: BytecodeObject,
    /// The source mapping as a string. See the source mapping definition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_map: Option<String>,
    /// If given, this is an unlinked object.
    #[serde(default)]
    pub link_references: BTreeMap<String, BTreeMap<String, Vec<Offsets>>>,
}

impl CompactBytecode {
    /// Returns a new `CompactBytecode` object that contains nothing, as it's the case for
    /// interfaces and standalone solidity files that don't contain any contract definitions
    pub fn empty() -> Self {
        Self { object: Default::default(), source_map: None, link_references: Default::default() }
    }

    /// Returns the parsed source map
    ///
    /// See also <https://docs.soliditylang.org/en/v0.8.10/internals/source_mappings.html>
    pub fn source_map(&self) -> Option<Result<SourceMap, SyntaxError>> {
        self.source_map.as_ref().map(|map| sourcemap::parse(map))
    }

    /// Tries to link the bytecode object with the `file` and `library` name.
    /// Replaces all library placeholders with the given address.
    ///
    /// Returns true if the bytecode object is fully linked, false otherwise
    /// This is a noop if the bytecode object is already fully linked.
    pub fn link(&mut self, file: &str, library: &str, address: Address) -> bool {
        if !self.object.is_unlinked() {
            return true;
        }

        if let Some((key, mut contracts)) = self.link_references.remove_entry(file) {
            if contracts.remove(library).is_some() {
                self.object.link(file, library, address);
            }
            if !contracts.is_empty() {
                self.link_references.insert(key, contracts);
            }
            if self.link_references.is_empty() {
                return self.object.resolve().is_some();
            }
        }
        false
    }

    /// Returns the bytes of the bytecode object.
    pub fn bytes(&self) -> Option<&Bytes> {
        self.object.as_bytes()
    }

    /// Returns the underlying `Bytes` if the object is a valid bytecode.
    pub fn into_bytes(self) -> Option<Bytes> {
        self.object.into_bytes()
    }
}

impl From<Bytecode> for CompactBytecode {
    fn from(bcode: Bytecode) -> Self {
        Self {
            object: bcode.object,
            source_map: bcode.source_map,
            link_references: bcode.link_references,
        }
    }
}

impl From<CompactBytecode> for Bytecode {
    fn from(bcode: CompactBytecode) -> Self {
        Self {
            object: bcode.object,
            source_map: bcode.source_map,
            link_references: bcode.link_references,
            function_debug_data: Default::default(),
            opcodes: Default::default(),
            generated_sources: Default::default(),
        }
    }
}

impl From<BytecodeObject> for Bytecode {
    fn from(object: BytecodeObject) -> Self {
        Self {
            object,
            function_debug_data: Default::default(),
            opcodes: Default::default(),
            source_map: Default::default(),
            generated_sources: Default::default(),
            link_references: Default::default(),
        }
    }
}

impl Bytecode {
    /// Returns the parsed source map
    ///
    /// See also <https://docs.soliditylang.org/en/v0.8.10/internals/source_mappings.html>
    pub fn source_map(&self) -> Option<Result<SourceMap, SyntaxError>> {
        self.source_map.as_ref().map(|map| sourcemap::parse(map))
    }

    /// Same as `Bytecode::link` but with fully qualified name (`file.sol:Math`)
    pub fn link_fully_qualified(&mut self, name: &str, addr: Address) -> bool {
        if let Some((file, lib)) = name.split_once(':') {
            self.link(file, lib, addr)
        } else {
            false
        }
    }

    /// Tries to link the bytecode object with the `file` and `library` name.
    /// Replaces all library placeholders with the given address.
    ///
    /// Returns true if the bytecode object is fully linked, false otherwise
    /// This is a noop if the bytecode object is already fully linked.
    pub fn link(&mut self, file: &str, library: &str, address: Address) -> bool {
        if !self.object.is_unlinked() {
            return true;
        }

        if let Some((key, mut contracts)) = self.link_references.remove_entry(file) {
            if contracts.remove(library).is_some() {
                self.object.link(file, library, address);
            }
            if !contracts.is_empty() {
                self.link_references.insert(key, contracts);
            }
            if self.link_references.is_empty() {
                return self.object.resolve().is_some();
            }
        }
        false
    }

    /// Links the bytecode object with all provided `(file, lib, addr)`
    pub fn link_all<I, S, T>(&mut self, libs: I) -> bool
    where
        I: IntoIterator<Item = (S, T, Address)>,
        S: AsRef<str>,
        T: AsRef<str>,
    {
        for (file, lib, addr) in libs.into_iter() {
            if self.link(file.as_ref(), lib.as_ref(), addr) {
                return true;
            }
        }
        false
    }

    /// Links the bytecode object with all provided `(fully_qualified, addr)`
    pub fn link_all_fully_qualified<I, S>(&mut self, libs: I) -> bool
    where
        I: IntoIterator<Item = (S, Address)>,
        S: AsRef<str>,
    {
        for (name, addr) in libs.into_iter() {
            if self.link_fully_qualified(name.as_ref(), addr) {
                return true;
            }
        }
        false
    }

    /// Returns a reference to the underlying `Bytes` if the object is a valid bytecode.
    pub fn bytes(&self) -> Option<&Bytes> {
        self.object.as_bytes()
    }

    /// Returns the underlying `Bytes` if the object is a valid bytecode.
    pub fn into_bytes(self) -> Option<Bytes> {
        self.object.into_bytes()
    }
}

/// Represents the bytecode of a contracts that might be not fully linked yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BytecodeObject {
    /// Fully linked bytecode object.
    #[serde(deserialize_with = "serde_helpers::deserialize_bytes")]
    Bytecode(Bytes),
    /// Bytecode as hex string that's not fully linked yet and contains library placeholders.
    #[serde(with = "serde_helpers::string_bytes")]
    Unlinked(String),
}

impl BytecodeObject {
    /// Returns a reference to the underlying `Bytes` if the object is a valid bytecode.
    pub fn as_bytes(&self) -> Option<&Bytes> {
        match self {
            Self::Bytecode(bytes) => Some(bytes),
            Self::Unlinked(_) => None,
        }
    }

    /// Returns the underlying `Bytes` if the object is a valid bytecode.
    pub fn into_bytes(self) -> Option<Bytes> {
        match self {
            Self::Bytecode(bytes) => Some(bytes),
            Self::Unlinked(_) => None,
        }
    }

    /// Returns the number of bytes of the fully linked bytecode.
    ///
    /// Returns `0` if this object is unlinked.
    pub fn bytes_len(&self) -> usize {
        self.as_bytes().map(|b| b.as_ref().len()).unwrap_or_default()
    }

    /// Returns a reference to the underlying `String` if the object is unlinked.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Bytecode(_) => None,
            Self::Unlinked(s) => Some(s.as_str()),
        }
    }

    /// Returns the unlinked `String` if the object is unlinked.
    pub fn into_unlinked(self) -> Option<String> {
        match self {
            Self::Bytecode(_) => None,
            Self::Unlinked(code) => Some(code),
        }
    }

    /// Whether this object is still unlinked.
    pub fn is_unlinked(&self) -> bool {
        matches!(self, Self::Unlinked(_))
    }

    /// Whether this object a valid bytecode.
    pub fn is_bytecode(&self) -> bool {
        matches!(self, Self::Bytecode(_))
    }

    /// Returns `true` if the object is a valid bytecode and not empty.
    ///
    /// Returns `false` if the object is a valid but empty or unlinked bytecode.
    pub fn is_non_empty_bytecode(&self) -> bool {
        self.as_bytes().map(|c| !c.0.is_empty()).unwrap_or_default()
    }

    /// Tries to resolve the unlinked string object a valid bytecode object in place.
    ///
    /// Returns the string if it is a valid
    pub fn resolve(&mut self) -> Option<&Bytes> {
        if let Self::Unlinked(unlinked) = self {
            if let Ok(linked) = hex::decode(unlinked) {
                *self = Self::Bytecode(linked.into());
            }
        }
        self.as_bytes()
    }

    /// Links using the fully qualified name of a library.
    ///
    /// The fully qualified library name is the path of its source file and the library name
    /// separated by `:` like `file.sol:Math`
    ///
    /// This will replace all occurrences of the library placeholder with the given address.
    ///
    /// See also: <https://docs.soliditylang.org/en/develop/using-the-compiler.html#library-linking>
    pub fn link_fully_qualified(&mut self, name: &str, addr: Address) -> &mut Self {
        if let Self::Unlinked(ref mut unlinked) = self {
            let place_holder = utils::library_hash_placeholder(name);
            // the address as hex without prefix
            let hex_addr = hex::encode(addr);

            // the library placeholder used to be the fully qualified name of the library instead of
            // the hash. This is also still supported by `solc` so we handle this as well
            let fully_qualified_placeholder = utils::library_fully_qualified_placeholder(name);

            *unlinked = unlinked
                .replace(&format!("__{fully_qualified_placeholder}__"), &hex_addr)
                .replace(&format!("__{place_holder}__"), &hex_addr)
        }
        self
    }

    /// Links using the `file` and `library` names as fully qualified name `<file>:<library>`.
    ///
    /// See [`link_fully_qualified`](Self::link_fully_qualified).
    pub fn link(&mut self, file: &str, library: &str, addr: Address) -> &mut Self {
        self.link_fully_qualified(&format!("{file}:{library}"), addr)
    }

    /// Links the bytecode object with all provided `(file, lib, addr)`.
    pub fn link_all<I, S, T>(&mut self, libs: I) -> &mut Self
    where
        I: IntoIterator<Item = (S, T, Address)>,
        S: AsRef<str>,
        T: AsRef<str>,
    {
        for (file, lib, addr) in libs.into_iter() {
            self.link(file.as_ref(), lib.as_ref(), addr);
        }
        self
    }

    /// Returns whether the bytecode contains a matching placeholder using the qualified name.
    pub fn contains_fully_qualified_placeholder(&self, name: &str) -> bool {
        if let Self::Unlinked(unlinked) = self {
            unlinked.contains(&utils::library_hash_placeholder(name))
                || unlinked.contains(&utils::library_fully_qualified_placeholder(name))
        } else {
            false
        }
    }

    /// Returns whether the bytecode contains a matching placeholder.
    pub fn contains_placeholder(&self, file: &str, library: &str) -> bool {
        self.contains_fully_qualified_placeholder(&format!("{file}:{library}"))
    }
}

// Returns an empty bytecode object
impl Default for BytecodeObject {
    fn default() -> Self {
        Self::Bytecode(Default::default())
    }
}

impl AsRef<[u8]> for BytecodeObject {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Bytecode(code) => code.as_ref(),
            Self::Unlinked(code) => code.as_bytes(),
        }
    }
}

/// This will serialize the bytecode data without a `0x` prefix, which the `ethers::types::Bytes`
/// adds by default.
///
/// This ensures that we serialize bytecode data in the same way as solc does, See also <https://github.com/gakonst/ethers-rs/issues/1422>
pub fn serialize_bytecode_without_prefix<S>(
    bytecode: &BytecodeObject,
    s: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    match bytecode {
        BytecodeObject::Bytecode(code) => s.serialize_str(&hex::encode(code)),
        BytecodeObject::Unlinked(code) => s.serialize_str(code.strip_prefix("0x").unwrap_or(code)),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeployedBytecode {
    #[serde(flatten)]
    pub bytecode: Option<Bytecode>,
    #[serde(
        default,
        rename = "immutableReferences",
        skip_serializing_if = "::std::collections::BTreeMap::is_empty"
    )]
    pub immutable_references: BTreeMap<String, Vec<Offsets>>,
}

impl DeployedBytecode {
    /// Returns a reference to the underlying `Bytes` if the object is a valid bytecode.
    pub fn bytes(&self) -> Option<&Bytes> {
        self.bytecode.as_ref().and_then(|bytecode| bytecode.object.as_bytes())
    }

    /// Returns the underlying `Bytes` if the object is a valid bytecode.
    pub fn into_bytes(self) -> Option<Bytes> {
        self.bytecode.and_then(|bytecode| bytecode.object.into_bytes())
    }
}

impl From<Bytecode> for DeployedBytecode {
    fn from(bcode: Bytecode) -> Self {
        Self { bytecode: Some(bcode), immutable_references: Default::default() }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactDeployedBytecode {
    #[serde(flatten)]
    pub bytecode: Option<CompactBytecode>,
    #[serde(
        default,
        rename = "immutableReferences",
        skip_serializing_if = "::std::collections::BTreeMap::is_empty"
    )]
    pub immutable_references: BTreeMap<String, Vec<Offsets>>,
}

impl CompactDeployedBytecode {
    /// Returns a new `CompactDeployedBytecode` object that contains nothing, as it's the case for
    /// interfaces and standalone solidity files that don't contain any contract definitions
    pub fn empty() -> Self {
        Self { bytecode: Some(CompactBytecode::empty()), immutable_references: Default::default() }
    }

    /// Returns a reference to the underlying `Bytes` if the object is a valid bytecode.
    pub fn bytes(&self) -> Option<&Bytes> {
        self.bytecode.as_ref().and_then(|bytecode| bytecode.object.as_bytes())
    }

    /// Returns the underlying `Bytes` if the object is a valid bytecode.
    pub fn into_bytes(self) -> Option<Bytes> {
        self.bytecode.and_then(|bytecode| bytecode.object.into_bytes())
    }

    /// Returns the parsed source map
    ///
    /// See also <https://docs.soliditylang.org/en/v0.8.10/internals/source_mappings.html>
    pub fn source_map(&self) -> Option<Result<SourceMap, SyntaxError>> {
        self.bytecode.as_ref().and_then(|bytecode| bytecode.source_map())
    }
}

impl From<DeployedBytecode> for CompactDeployedBytecode {
    fn from(bcode: DeployedBytecode) -> Self {
        Self {
            bytecode: bcode.bytecode.map(|d_bcode| d_bcode.into()),
            immutable_references: bcode.immutable_references,
        }
    }
}

impl From<CompactDeployedBytecode> for DeployedBytecode {
    fn from(bcode: CompactDeployedBytecode) -> Self {
        Self {
            bytecode: bcode.bytecode.map(|d_bcode| d_bcode.into()),
            immutable_references: bcode.immutable_references,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{ConfigurableContractArtifact, ContractBytecode};

    #[test]
    fn test_empty_bytecode() {
        let empty = r#"
        {
  "abi": [],
  "bytecode": {
    "object": "0x",
    "linkReferences": {}
  },
  "deployedBytecode": {
    "object": "0x",
    "linkReferences": {}
  }
  }
        "#;

        let artifact: ConfigurableContractArtifact = serde_json::from_str(empty).unwrap();
        let contract = artifact.into_contract_bytecode();
        let bytecode: ContractBytecode = contract.into();
        let bytecode = bytecode.unwrap();
        assert!(!bytecode.bytecode.object.is_unlinked());
    }
}
