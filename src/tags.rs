use std::cell::RefCell;

use dirs::data_dir;
use smol::{
    fs::{File, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    lock::Mutex,
};
use tracing::debug;

pub struct Tags {
    tags: RefCell<Vec<String>>,
    file: Mutex<File>,
}

impl Tags {
    pub async fn new(name: &str) -> Self {
        let mut file_path = data_dir().expect("Could not find data directory");
        file_path.push("code-statistics");
        file_path.push(name);

        let mut file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .append(true)
            .open(file_path)
            .await
            .expect("Failed to open tag path");

        let mut reader = BufReader::new(&mut file);
        let mut tags = Vec::new();

        let mut line = String::new();
        while reader
            .read_line(&mut line)
            .await
            .expect("Failed to read from tag file")
            != 0
        {
            tags.push(std::mem::take(&mut line).trim().to_string());
        }

        debug!(?tags);

        Self {
            tags: RefCell::new(tags),
            file: Mutex::new(file),
        }
    }

    pub async fn get(&self, language: &str) -> usize {
        let pos = {
            let tags = self.tags.borrow();
            tags.iter()
                .position(|known_language| known_language == language)
        };

        debug!(?pos);

        if let Some(pos) = pos {
            pos
        } else {
            let mut file = self.file.lock().await;
            file.write_all(language.as_bytes())
                .await
                .expect("Failed to write to tag file");
            file.write_all("\n".as_bytes())
                .await
                .expect("Failed to write to tag file");

            let mut tags = self.tags.borrow_mut();

            tags.push(language.to_owned());

            tags.len() - 1
        }
    }
}
