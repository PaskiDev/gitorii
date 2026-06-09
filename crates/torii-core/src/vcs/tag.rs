use crate::error::Result;
use git2::Repository;

pub struct TagManager<'repo> {
    repo: &'repo Repository,
}

impl<'repo> TagManager<'repo> {
    pub fn new(repo: &'repo Repository) -> Self {
        Self { repo }
    }

    pub fn create_tag(&self, name: &str, message: Option<&str>) -> Result<()> {
        let head = self.repo.head()?;
        let target = head.peel_to_commit()?;

        if let Some(msg) = message {
            // Annotated tag
            let sig = crate::core::resolve_signature(self.repo)?;
            self.repo.tag(name, target.as_object(), &sig, msg, false)?;
        } else {
            // Lightweight tag
            self.repo.tag_lightweight(name, target.as_object(), false)?;
        }

        Ok(())
    }

    pub fn list_tags(&self) -> Result<Vec<String>> {
        let tags = self.repo.tag_names(None)?;
        let mut tag_list = Vec::new();

        for tag_name in tags.iter().flatten() {
            tag_list.push(tag_name.to_string());
        }

        tag_list.sort();
        Ok(tag_list)
    }

    pub fn delete_tag(&self, name: &str) -> Result<()> {
        self.repo.tag_delete(name)?;
        Ok(())
    }

    pub fn show_tag(&self, name: &str) -> Result<()> {
        let tag_ref = self.repo.find_reference(&format!("refs/tags/{}", name))?;
        let tag_obj = tag_ref.peel_to_tag();

        if let Ok(tag) = tag_obj {
            // Annotated tag
            println!("Tag: {}", name);
            if let Some(tagger) = tag.tagger() {
                println!(
                    "Tagger: {} <{}>",
                    tagger.name().unwrap_or(""),
                    tagger.email().unwrap_or("")
                );
            }
            if let Some(message) = tag.message() {
                println!("Message: {}", message);
            }
        } else {
            // Lightweight tag
            let commit = tag_ref.peel_to_commit()?;
            println!("Tag: {} (lightweight)", name);
            println!("Commit: {}", commit.id());
            if let Some(msg) = commit.message() {
                println!("Message: {}", msg.lines().next().unwrap_or(""));
            }
        }

        Ok(())
    }
}
