use crate::utils::{CMakePackage, CMakePackageFrom, FileType};
use std::sync::LazyLock;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use crate::Url;

use super::{get_version, CMAKECONFIG, CMAKECONFIGVERSION, CMAKEREGEX};

const LIBS: [&str; 4] = ["lib", "lib32", "lib64", "share"];

pub static CMAKE_PACKAGES: LazyLock<Vec<CMakePackage>> =
    LazyLock::new(|| get_cmake_message().into_values().collect());

pub static CMAKE_PACKAGES_WITHKEY: LazyLock<HashMap<String, CMakePackage>> =
    LazyLock::new(get_cmake_message);

fn get_prefix() -> Option<String> {
    if let Ok(mystem_prefix) = std::env::var("MSYSTEM_PREFIX") {
        return Some(mystem_prefix);
    }
    std::env::var("CMAKE_PREFIX_PATH").ok()
}

fn get_available_libs(prefix: &str) -> Vec<PathBuf> {
    let mut ava: Vec<PathBuf> = Vec::new();
    let root_prefix = Path::new(&prefix);
    for lib in LIBS {
        let p = root_prefix.join(lib).join("cmake");
        if p.exists() {
            ava.push(p);
        }
    }
    ava
}

fn safe_canonicalize<P: AsRef<Path>>(path: P) -> std::io::Result<PathBuf> {
    use path_absolutize::Absolutize;
    Ok(path.as_ref().absolutize()?.into_owned())
}

#[inline]
fn get_cmake_message() -> HashMap<String, CMakePackage> {
    let Some(prefix) = get_prefix() else {
        return HashMap::new();
    };
    get_cmake_message_with_prefix(&prefix)
}

fn get_cmake_message_with_prefix(prefix: &str) -> HashMap<String, CMakePackage> {
    let mut packages: HashMap<String, CMakePackage> = HashMap::new();
    if let Ok(paths) = glob::glob(&format!("{prefix}/share/*/cmake/")) {
        for path in paths.flatten() {
            let Ok(files) = glob::glob(&format!("{}/*.cmake", path.to_string_lossy())) else {
                continue;
            };
            let mut tojump: Vec<PathBuf> = vec![];
            let mut version: Option<String> = None;
            let mut ispackage = false;
            for f in files.flatten() {
                tojump.push(safe_canonicalize(&f).unwrap());
                if CMAKECONFIG.is_match(f.to_str().unwrap()) {
                    ispackage = true;
                }
                if CMAKECONFIGVERSION.is_match(f.to_str().unwrap()) {
                    if let Ok(context) = fs::read_to_string(&f) {
                        version = get_version(&context);
                    }
                }
            }
            if ispackage {
                let filepath = Url::from_file_path(&path).unwrap();
                let packagename = path
                    .parent()
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_str()
                    .unwrap();
                packages
                    .entry(packagename.to_string())
                    .or_insert_with(|| CMakePackage {
                        name: packagename.to_string(),
                        filetype: FileType::Dir,
                        filepath,
                        version,
                        tojump,
                        from: CMakePackageFrom::System,
                    });
            }
        }
    }

    for lib in get_available_libs(prefix) {
        let Ok(paths) = std::fs::read_dir(lib) else {
            continue;
        };
        for path in paths.flatten() {
            let mut version: Option<String> = None;
            let mut tojump: Vec<PathBuf> = vec![];
            let pathname = path.file_name().to_str().unwrap().to_string();

            let packagepath = Url::from_file_path(path.path()).unwrap();
            let (packagetype, packagename) = {
                if path.metadata().is_ok_and(|data| data.is_dir()) {
                    let Ok(paths) = std::fs::read_dir(path.path()) else {
                        continue;
                    };
                    for path in paths.flatten() {
                        let filepath = safe_canonicalize(path.path()).unwrap();
                        if path.metadata().unwrap().is_file() {
                            let filename = path.file_name().to_str().unwrap().to_string();
                            if CMAKEREGEX.is_match(&filename) {
                                tojump.push(filepath.clone());
                                if CMAKECONFIGVERSION.is_match(&filename) {
                                    if let Ok(context) = fs::read_to_string(&filepath) {
                                        version = get_version(&context);
                                    }
                                }
                            }
                        }
                    }

                    (FileType::Dir, pathname)
                } else {
                    tojump.push(safe_canonicalize(path.path()).unwrap());
                    let pathname = pathname.split('.').collect::<Vec<&str>>()[0].to_string();
                    (FileType::File, pathname)
                }
            };
            packages
                .entry(packagename.clone())
                .or_insert_with(|| CMakePackage {
                    name: packagename,
                    filetype: packagetype,
                    filepath: packagepath,
                    version,
                    tojump,
                    from: CMakePackageFrom::System,
                });
        }
    }
    packages
}

#[test]
fn test_package_search() {
    use std::fs;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;
    let dir = tempdir().unwrap();

    let share_dir = dir.path().join("share");
    let cmake_dir = share_dir.join("cmake");
    let vulkan_dir = cmake_dir.join("VulkanHeaders");
    fs::create_dir_all(&vulkan_dir).unwrap();
    let vulkan_config_cmake = vulkan_dir.join("VulkanHeadersConfig.cmake");

    File::create(&vulkan_config_cmake).unwrap();
    let vulkan_config_version_cmake = vulkan_dir.join("VulkanHeadersConfigVersion.cmake");
    let mut vulkan_config_version_file = File::create(&vulkan_config_version_cmake).unwrap();
    writeln!(
        vulkan_config_version_file,
        r#"set(PACKAGE_VERSION "1.3.295")"#
    )
    .unwrap();

    let ecm_dir = share_dir.join("ECM").join("cmake");
    fs::create_dir_all(&ecm_dir).unwrap();
    let ecm_config_cmake = ecm_dir.join("ECMConfig.cmake");
    File::create(&ecm_config_cmake).unwrap();
    let ecm_config_version_cmake = ecm_dir.join("ECMConfigVersion.cmake");
    let mut ecm_config_version_file = File::create(&ecm_config_version_cmake).unwrap();
    writeln!(ecm_config_version_file, r#"set(PACKAGE_VERSION "6.5.0")"#).unwrap();

    let prefix = safe_canonicalize(dir.path())
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let target = HashMap::from_iter([
        (
            "VulkanHeaders".to_string(),
            CMakePackage {
                name: "VulkanHeaders".to_string(),
                filetype: FileType::Dir,
                filepath: Url::from_file_path(vulkan_dir).unwrap(),
                version: Some("1.3.295".to_string()),
                tojump: vec![vulkan_config_cmake, vulkan_config_version_cmake],
                from: CMakePackageFrom::System,
            },
        ),
        (
            "ECM".to_string(),
            CMakePackage {
                name: "ECM".to_string(),
                filetype: FileType::Dir,
                filepath: Url::from_file_path(ecm_dir).unwrap(),
                version: Some("6.5.0".to_string()),
                tojump: vec![ecm_config_cmake, ecm_config_version_cmake],
                from: CMakePackageFrom::System,
            },
        ),
    ]);
    assert_eq!(get_cmake_message_with_prefix(&prefix), target);
}
