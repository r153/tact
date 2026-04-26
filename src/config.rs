use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

const CONFIG_FILE_NAME: &str = "config.yaml";

/// アプリケーションの設定を保持する構造体（お気に入りリストなど）
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub favorite: Vec<String>,
}

impl Config {
    /// 設定ファイルのデフォルトパスを返す
    pub fn default_path() -> PathBuf {
        PathBuf::from(CONFIG_FILE_NAME)
    }

    /// 既定パスから設定を読み、設定とパスを返す
    pub fn load_default() -> io::Result<(Self, PathBuf)> {
        let path = Self::default_path();
        let config = Self::load_from_path(&path)?;
        Ok((config, path))
    }

    /// 読み込みに失敗した場合はデフォルト値で初期化する
    pub fn load_or_default() -> (Self, PathBuf) {
        match Self::load_default() {
            Ok(res) => res,
            Err(_) => (Self::default(), Self::default_path()),
        }
    }

    /// 指定パスから設定ファイルを読み込む
    fn load_from_path(path: &Path) -> io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(contents) => {
                let cfg = serde_yaml::from_str(&contents)
                    .map_err(|err| io::Error::new(ErrorKind::InvalidData, err))?;
                Ok(cfg)
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(Self::default()),
            Err(err) => Err(err),
        }
    }

    /// 設定内容を YAML として保存する
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let yaml = serde_yaml::to_string(self)
            .map_err(|err| io::Error::new(ErrorKind::InvalidData, err))?;
        fs::write(path, yaml)
    }

    /// 指定パスがお気に入りに登録済みか判定する
    pub fn is_favorite(&self, path: &str) -> bool {
        self.favorite.iter().any(|p| p == path)
    }

    /// お気に入りへ追加する（既存なら false）
    pub fn add_favorite(&mut self, path: &str) -> bool {
        if self.is_favorite(path) {
            return false;
        }
        self.favorite.push(path.to_string());
        true
    }

    /// お気に入りから削除する
    pub fn remove_favorite(&mut self, path: &str) -> bool {
        let len_before = self.favorite.len();
        self.favorite.retain(|p| p != path);
        len_before != self.favorite.len()
    }
}
