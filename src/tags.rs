use dirs::data_dir;
use smol::{
    fs::{File, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
};

pub struct Tags {
    tags: Vec<String>,
    file: File,
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
            tags.push(std::mem::take(&mut line));
        }

        Self { tags, file }
    }

    pub async fn get(&mut self, language: &str) -> usize {
        if let Some(pos) = self
            .tags
            .iter()
            .position(|known_language| known_language == language)
        {
            pos
        } else {
            self.file
                .write_all(language.as_bytes())
                .await
                .expect("Failed to write to tag file");
            self.file
                .write_all("\n".as_bytes())
                .await
                .expect("Failed to write to tag file");

            self.tags.push(language.to_owned());

            self.tags.len() - 1
        }
    }
}
