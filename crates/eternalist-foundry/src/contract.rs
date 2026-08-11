use std::{collections::BTreeSet, fmt, fs, path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    error::{Error, io},
};

pub const SCHEMA: u32 = 1;

pub fn workspace_of(contract: &Path) -> &Path {
    contract
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Contract {
    pub schema: u32,
    pub product: Product,
    pub coordinates: Coordinates,
    #[serde(default)]
    pub exclusions: Vec<Exclusion>,
    #[serde(default)]
    pub delivery: DeliveryPolicy,
    #[serde(default)]
    pub trust: TrustPolicy,
    pub proofs: Vec<Proof>,
}

impl Contract {
    pub fn load(path: &Path) -> Result<Self> {
        let source = fs::read_to_string(path).map_err(|source| io("read", path, source))?;
        let contract = toml::from_str::<Self>(&source).map_err(|source| Error::Toml {
            path: path.to_path_buf(),
            source,
        })?;
        contract.validate()?;
        Ok(contract)
    }

    pub fn validate(&self) -> Result<()> {
        require(self.schema == SCHEMA, || {
            format!("schema {} is unsupported; expected {SCHEMA}", self.schema)
        })?;
        self.product.validate()?;
        self.coordinates.validate()?;
        self.validate_envelope()?;
        self.validate_delivery()?;
        self.validate_proofs()
    }

    pub fn release_coordinates(&self) -> impl Iterator<Item = Coordinate> + '_ {
        self.coordinates.release_tested.iter().copied()
    }

    pub fn carried_coordinates(&self) -> impl Iterator<Item = Coordinate> + '_ {
        self.coordinates
            .release_tested
            .iter()
            .chain(&self.coordinates.supported)
            .copied()
    }

    pub fn release_platforms(&self) -> BTreeSet<Platform> {
        self.release_coordinates()
            .map(Coordinate::platform)
            .collect()
    }

    pub fn proof(&self, name: &str) -> Option<&Proof> {
        self.proofs.iter().find(|proof| proof.name == name)
    }

    fn validate_envelope(&self) -> Result<()> {
        let carried = self.carried_coordinates().collect::<BTreeSet<_>>();
        let excluded = self
            .exclusions
            .iter()
            .map(|exclusion| exclusion.coordinate)
            .collect::<BTreeSet<_>>();

        ensure_disjoint("carried coordinate", &carried, "exclusion", &excluded)?;
        for exclusion in &self.exclusions {
            require(exclusion.reason.trim().len() >= 12, || {
                format!(
                    "exclusion `{}` needs a concrete reason of at least 12 characters",
                    exclusion.coordinate
                )
            })?;
        }

        let exclusions = self
            .exclusions
            .iter()
            .map(|exclusion| exclusion.coordinate)
            .collect::<Vec<_>>();
        reject_duplicates("exclusion coordinate", &exclusions)?;

        for coordinate in self.product.profile.baseline() {
            require(
                carried.contains(coordinate) || excluded.contains(coordinate),
                || {
                    format!(
                        "baseline coordinate `{coordinate}` is neither carried nor explicitly excluded"
                    )
                },
            )?;
        }

        if self.product.profile != Profile::PlatformBound {
            for coordinate in &carried {
                require(self.product.profile.baseline().contains(coordinate), || {
                    format!(
                        "coordinate `{coordinate}` does not belong to profile `{}`",
                        self.product.profile
                    )
                })?;
            }
        }
        Ok(())
    }

    fn validate_delivery(&self) -> Result<()> {
        for platform in self.release_platforms() {
            if self.product.profile == Profile::RustLibrary {
                continue;
            }
            let delivery = self.delivery.for_platform(platform).ok_or_else(|| {
                Error::Contract(format!(
                    "release-tested platform `{platform}` has no delivery policy"
                ))
            })?;
            require(delivery.admits(platform), || {
                format!("delivery `{delivery}` cannot inhabit `{platform}`")
            })?;
            if matches!(platform, Platform::Macos | Platform::Windows) {
                require(self.trust.for_platform(platform).is_some(), || {
                    format!("delivery on `{platform}` has no explicit trust state")
                })?;
            }
        }
        Ok(())
    }

    fn validate_proofs(&self) -> Result<()> {
        require(!self.proofs.is_empty(), || "proof list is empty".to_owned())?;
        let names = self
            .proofs
            .iter()
            .map(|proof| proof.name.clone())
            .collect::<Vec<_>>();
        reject_duplicates("proof name", &names)?;
        for proof in &self.proofs {
            proof.validate(self)?;
        }

        self.require_global_law(Law::Source)?;
        self.require_global_law(Law::Security)?;
        if self.product.profile != Profile::PlatformBound {
            self.require_global_law(Law::SourcePackage)?;
        }

        for coordinate in &self.coordinates.supported {
            self.require_coordinate_law(*coordinate, Law::Host)?;
        }
        for coordinate in &self.coordinates.release_tested {
            self.require_coordinate_law(*coordinate, Law::Host)?;
            match self.product.profile {
                Profile::NativeGui => {
                    self.require_coordinate_law(*coordinate, Law::FirstPresent)?;
                    self.require_coordinate_law(*coordinate, Law::Lifecycle)?;
                    if coordinate.display() == Some(DisplayBackend::X11) {
                        self.require_coordinate_law(*coordinate, Law::NativeAcceptance)?;
                    }
                }
                Profile::PortableCli | Profile::PlatformBound => {
                    self.require_coordinate_law(*coordinate, Law::Lifecycle)?;
                }
                Profile::RustLibrary => {}
            }
        }
        self.require_delivery_artifacts()
    }

    fn require_delivery_artifacts(&self) -> Result<()> {
        let release = self.release_coordinates().collect::<BTreeSet<_>>();
        for platform in self.release_platforms() {
            let Some(delivery) = self.delivery.for_platform(platform) else {
                continue;
            };
            if !delivery.produces_artifact() {
                continue;
            }
            require(
                self.proofs.iter().any(|proof| {
                    proof.laws.contains(&Law::Artifact)
                        && proof.coordinates.iter().any(|coordinate| {
                            release.contains(coordinate) && coordinate.platform() == platform
                        })
                }),
                || {
                    format!(
                        "delivery `{delivery}` on `{platform}` has no release-tested artifact proof"
                    )
                },
            )?;
        }
        Ok(())
    }

    fn require_global_law(&self, law: Law) -> Result<()> {
        require(
            self.proofs
                .iter()
                .any(|proof| proof.coordinates.is_empty() && proof.laws.contains(&law)),
            || format!("no global proof discharges `{law}`"),
        )
    }

    fn require_coordinate_law(&self, coordinate: Coordinate, law: Law) -> Result<()> {
        require(
            self.proofs
                .iter()
                .any(|proof| proof.coordinates.contains(&coordinate) && proof.laws.contains(&law)),
            || format!("coordinate `{coordinate}` has no proof of `{law}`"),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Product {
    pub name: String,
    pub package: String,
    #[serde(default)]
    pub binary: Option<String>,
    #[serde(default)]
    pub identifier: Option<String>,
    pub profile: Profile,
}

impl Product {
    fn validate(&self) -> Result<()> {
        for (field, value) in [("name", &self.name), ("package", &self.package)] {
            require(!value.trim().is_empty(), || {
                format!("product.{field} is empty")
            })?;
        }
        require(
            self.profile == Profile::RustLibrary || self.binary.is_some(),
            || format!("profile `{}` requires product.binary", self.profile),
        )
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Profile {
    NativeGui,
    PortableCli,
    RustLibrary,
    PlatformBound,
}

impl Profile {
    pub const fn baseline(self) -> &'static [Coordinate] {
        match self {
            Self::NativeGui => &[
                Coordinate::LinuxX86_64X11Vulkan,
                Coordinate::LinuxX86_64WaylandVulkan,
                Coordinate::MacosAarch64Metal,
                Coordinate::MacosX86_64Metal,
                Coordinate::WindowsX86_64Dx12,
            ],
            Self::PortableCli | Self::RustLibrary => &[
                Coordinate::LinuxX86_64,
                Coordinate::MacosAarch64,
                Coordinate::MacosX86_64,
                Coordinate::WindowsX86_64,
            ],
            Self::PlatformBound => &[],
        }
    }
}

impl fmt::Display for Profile {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NativeGui => "native-gui",
            Self::PortableCli => "portable-cli",
            Self::RustLibrary => "rust-library",
            Self::PlatformBound => "platform-bound",
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Coordinates {
    #[serde(default)]
    pub release_tested: Vec<Coordinate>,
    #[serde(default)]
    pub supported: Vec<Coordinate>,
}

impl Coordinates {
    fn validate(&self) -> Result<()> {
        require(!self.release_tested.is_empty(), || {
            "coordinates.release-tested is empty".to_owned()
        })?;
        reject_duplicates("release-tested coordinate", &self.release_tested)?;
        reject_duplicates("supported coordinate", &self.supported)?;
        let release = self.release_tested.iter().copied().collect::<BTreeSet<_>>();
        let supported = self.supported.iter().copied().collect::<BTreeSet<_>>();
        ensure_disjoint("release-tested", &release, "supported", &supported)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Coordinate {
    #[serde(rename = "linux-x86_64-x11-vulkan")]
    LinuxX86_64X11Vulkan,
    #[serde(rename = "linux-x86_64-wayland-vulkan")]
    LinuxX86_64WaylandVulkan,
    #[serde(rename = "macos-aarch64-metal")]
    MacosAarch64Metal,
    #[serde(rename = "macos-x86_64-metal")]
    MacosX86_64Metal,
    #[serde(rename = "windows-x86_64-dx12")]
    WindowsX86_64Dx12,
    #[serde(rename = "linux-x86_64")]
    LinuxX86_64,
    #[serde(rename = "macos-aarch64")]
    MacosAarch64,
    #[serde(rename = "macos-x86_64")]
    MacosX86_64,
    #[serde(rename = "windows-x86_64")]
    WindowsX86_64,
}

impl Coordinate {
    pub const fn platform(self) -> Platform {
        match self {
            Self::LinuxX86_64X11Vulkan | Self::LinuxX86_64WaylandVulkan | Self::LinuxX86_64 => {
                Platform::Linux
            }
            Self::MacosAarch64Metal
            | Self::MacosX86_64Metal
            | Self::MacosAarch64
            | Self::MacosX86_64 => Platform::Macos,
            Self::WindowsX86_64Dx12 | Self::WindowsX86_64 => Platform::Windows,
        }
    }

    pub const fn os(self) -> &'static str {
        match self.platform() {
            Platform::Linux => "linux",
            Platform::Macos => "macos",
            Platform::Windows => "windows",
        }
    }

    pub const fn arch(self) -> &'static str {
        match self {
            Self::MacosAarch64Metal | Self::MacosAarch64 => "aarch64",
            _ => "x86_64",
        }
    }

    pub const fn display(self) -> Option<DisplayBackend> {
        match self {
            Self::LinuxX86_64X11Vulkan => Some(DisplayBackend::X11),
            Self::LinuxX86_64WaylandVulkan => Some(DisplayBackend::Wayland),
            _ => None,
        }
    }

    pub const fn runner(self) -> &'static str {
        match self {
            Self::MacosAarch64Metal | Self::MacosAarch64 => "macos-15",
            Self::MacosX86_64Metal | Self::MacosX86_64 => "macos-15-intel",
            Self::WindowsX86_64Dx12 | Self::WindowsX86_64 => "windows-2025",
            _ => "ubuntu-24.04",
        }
    }

    pub const fn target_triple(self) -> &'static str {
        match self {
            Self::LinuxX86_64X11Vulkan | Self::LinuxX86_64WaylandVulkan | Self::LinuxX86_64 => {
                "x86_64-unknown-linux-gnu"
            }
            Self::MacosAarch64Metal | Self::MacosAarch64 => "aarch64-apple-darwin",
            Self::MacosX86_64Metal | Self::MacosX86_64 => "x86_64-apple-darwin",
            Self::WindowsX86_64Dx12 | Self::WindowsX86_64 => "x86_64-pc-windows-msvc",
        }
    }

    pub fn inhabits_current_host(self) -> bool {
        self.inhabits(std::env::consts::OS, std::env::consts::ARCH)
    }

    pub fn inhabits(self, os: &str, arch: &str) -> bool {
        self.os() == os && self.arch() == arch
    }
}

impl fmt::Display for Coordinate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::LinuxX86_64X11Vulkan => "linux-x86_64-x11-vulkan",
            Self::LinuxX86_64WaylandVulkan => "linux-x86_64-wayland-vulkan",
            Self::MacosAarch64Metal => "macos-aarch64-metal",
            Self::MacosX86_64Metal => "macos-x86_64-metal",
            Self::WindowsX86_64Dx12 => "windows-x86_64-dx12",
            Self::LinuxX86_64 => "linux-x86_64",
            Self::MacosAarch64 => "macos-aarch64",
            Self::MacosX86_64 => "macos-x86_64",
            Self::WindowsX86_64 => "windows-x86_64",
        })
    }
}

impl FromStr for Coordinate {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "linux-x86_64-x11-vulkan" => Ok(Self::LinuxX86_64X11Vulkan),
            "linux-x86_64-wayland-vulkan" => Ok(Self::LinuxX86_64WaylandVulkan),
            "macos-aarch64-metal" => Ok(Self::MacosAarch64Metal),
            "macos-x86_64-metal" => Ok(Self::MacosX86_64Metal),
            "windows-x86_64-dx12" => Ok(Self::WindowsX86_64Dx12),
            "linux-x86_64" => Ok(Self::LinuxX86_64),
            "macos-aarch64" => Ok(Self::MacosAarch64),
            "macos-x86_64" => Ok(Self::MacosX86_64),
            "windows-x86_64" => Ok(Self::WindowsX86_64),
            _ => Err(Error::Contract(format!("unknown coordinate `{value}`"))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Linux,
    Macos,
    Windows,
}

impl fmt::Display for Platform {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Linux => "linux",
            Self::Macos => "macos",
            Self::Windows => "windows",
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DisplayBackend {
    X11,
    Wayland,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exclusion {
    pub coordinate: Coordinate,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveryPolicy {
    #[serde(default)]
    pub linux: Option<Delivery>,
    #[serde(default)]
    pub macos: Option<Delivery>,
    #[serde(default)]
    pub windows: Option<Delivery>,
}

impl DeliveryPolicy {
    pub const fn for_platform(&self, platform: Platform) -> Option<Delivery> {
        match platform {
            Platform::Linux => self.linux,
            Platform::Macos => self.macos,
            Platform::Windows => self.windows,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Delivery {
    Cargo,
    Dmg,
    NsisCurrentUser,
    DistShell,
    DistPowershell,
    GithubArchive,
}

impl Delivery {
    const fn admits(self, platform: Platform) -> bool {
        match self {
            Self::Cargo | Self::GithubArchive => true,
            Self::Dmg => matches!(platform, Platform::Macos),
            Self::NsisCurrentUser | Self::DistPowershell => matches!(platform, Platform::Windows),
            Self::DistShell => matches!(platform, Platform::Linux | Platform::Macos),
        }
    }

    pub const fn produces_artifact(self) -> bool {
        !matches!(self, Self::Cargo)
    }
}

impl fmt::Display for Delivery {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cargo => "cargo",
            Self::Dmg => "dmg",
            Self::NsisCurrentUser => "nsis-current-user",
            Self::DistShell => "dist-shell",
            Self::DistPowershell => "dist-powershell",
            Self::GithubArchive => "github-archive",
        })
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TrustPolicy {
    #[serde(default)]
    pub macos: Option<Trust>,
    #[serde(default)]
    pub windows: Option<Trust>,
}

impl TrustPolicy {
    pub const fn for_platform(&self, platform: Platform) -> Option<Trust> {
        match platform {
            Platform::Linux => Some(Trust::NotApplicable),
            Platform::Macos => self.macos,
            Platform::Windows => self.windows,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Trust {
    NotApplicable,
    UnsignedIncubation,
    Signed,
    Notarized,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Proof {
    pub name: String,
    pub laws: Vec<Law>,
    pub run: Vec<String>,
    #[serde(default)]
    pub coordinates: Vec<Coordinate>,
    #[serde(default = "default_timeout")]
    pub timeout_minutes: u16,
}

impl Proof {
    fn validate(&self, contract: &Contract) -> Result<()> {
        require(valid_slug(&self.name), || {
            format!(
                "proof name `{}` must contain only lowercase ASCII, digits, and single hyphens",
                self.name
            )
        })?;
        require(!self.laws.is_empty(), || {
            format!("proof `{}` discharges no laws", self.name)
        })?;
        reject_duplicates(&format!("law in proof `{}`", self.name), &self.laws)?;
        require(
            !self.run.is_empty() && self.run.iter().all(|part| !part.is_empty()),
            || format!("proof `{}` has an empty command", self.name),
        )?;
        reject_duplicates(
            &format!("coordinate in proof `{}`", self.name),
            &self.coordinates,
        )?;
        require((1..=360).contains(&self.timeout_minutes), || {
            format!(
                "proof `{}` timeout {} is outside 1..=360 minutes",
                self.name, self.timeout_minutes
            )
        })?;

        let global = self.laws.iter().any(|law| law.is_global());
        let coordinate = self.laws.iter().any(|law| !law.is_global());
        require(!(global && coordinate), || {
            format!("proof `{}` mixes global and coordinate laws", self.name)
        })?;
        require(global == self.coordinates.is_empty(), || {
            format!(
                "proof `{}` must {} coordinates",
                self.name,
                if global { "omit" } else { "declare" }
            )
        })?;

        let carried = contract.carried_coordinates().collect::<BTreeSet<_>>();
        for proof_coordinate in &self.coordinates {
            require(carried.contains(proof_coordinate), || {
                format!(
                    "proof `{}` names uncarried coordinate `{proof_coordinate}`",
                    self.name
                )
            })?;
        }
        Ok(())
    }

    pub fn setup(&self, coordinate: Option<Coordinate>) -> Setup {
        let needs_display = self.laws.iter().any(|law| law.needs_display());
        if !needs_display {
            return Setup::Ordinary;
        }
        match coordinate.and_then(Coordinate::display) {
            Some(DisplayBackend::X11) => Setup::X11,
            Some(DisplayBackend::Wayland) => Setup::Wayland,
            None => Setup::Ordinary,
        }
    }
}

const fn default_timeout() -> u16 {
    45
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Law {
    Source,
    Security,
    SourcePackage,
    Host,
    FirstPresent,
    NativeAcceptance,
    Lifecycle,
    Artifact,
}

impl Law {
    const fn is_global(self) -> bool {
        matches!(self, Self::Source | Self::Security | Self::SourcePackage)
    }

    const fn needs_display(self) -> bool {
        matches!(
            self,
            Self::FirstPresent | Self::NativeAcceptance | Self::Lifecycle
        )
    }
}

impl fmt::Display for Law {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Source => "source",
            Self::Security => "security",
            Self::SourcePackage => "source-package",
            Self::Host => "host",
            Self::FirstPresent => "first-present",
            Self::NativeAcceptance => "native-acceptance",
            Self::Lifecycle => "lifecycle",
            Self::Artifact => "artifact",
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Setup {
    Ordinary,
    X11,
    Wayland,
}

impl fmt::Display for Setup {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ordinary => "ordinary",
            Self::X11 => "x11",
            Self::Wayland => "wayland",
        })
    }
}

fn valid_slug(value: &str) -> bool {
    let mut prior_hyphen = true;
    for byte in value.bytes() {
        let hyphen = byte == b'-';
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || hyphen)
            || (hyphen && prior_hyphen)
        {
            return false;
        }
        prior_hyphen = hyphen;
    }
    !value.is_empty() && !prior_hyphen
}

fn reject_duplicates<T>(name: &str, values: &[T]) -> Result<()>
where
    T: Clone + fmt::Display + Ord,
{
    let mut seen = BTreeSet::new();
    for value in values {
        require(seen.insert(value.clone()), || {
            format!("duplicate {name} `{value}`")
        })?;
    }
    Ok(())
}

fn ensure_disjoint<T>(
    left_name: &str,
    left: &BTreeSet<T>,
    right_name: &str,
    right: &BTreeSet<T>,
) -> Result<()>
where
    T: fmt::Display + Ord,
{
    if let Some(value) = left.intersection(right).next() {
        return Err(Error::Contract(format!(
            "{left_name} `{value}` also appears as {right_name}"
        )));
    }
    Ok(())
}

fn require(condition: bool, message: impl FnOnce() -> String) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Contract(message()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_coordinate_round_trips_through_its_name() {
        let profiles = [
            Profile::NativeGui,
            Profile::PortableCli,
            Profile::RustLibrary,
        ];
        for coordinate in profiles
            .into_iter()
            .flat_map(Profile::baseline)
            .copied()
            .collect::<BTreeSet<_>>()
        {
            assert_eq!(
                coordinate
                    .to_string()
                    .parse::<Coordinate>()
                    .expect("parse coordinate"),
                coordinate
            );
        }
    }

    #[test]
    fn slug_rejects_ambiguous_separators() {
        assert!(valid_slug("native-host-2"));
        for alien in ["", "Native", "native_host", "native--host", "native-"] {
            assert!(!valid_slug(alien), "accepted {alien}");
        }
    }

    #[test]
    fn one_universal_artifact_proof_covers_the_macos_delivery() {
        let contract =
            toml::from_str::<Contract>(include_str!("../../../tests/fixtures/native-gui.toml"))
                .expect("parse native GUI fixture");
        contract.validate().expect("validate native GUI fixture");
        let artifact = contract
            .proof("artifact-macos")
            .expect("macOS artifact proof");
        assert_eq!(artifact.coordinates, [Coordinate::MacosAarch64Metal]);
        assert!(
            contract
                .coordinates
                .release_tested
                .contains(&Coordinate::MacosX86_64Metal)
        );
    }
}
