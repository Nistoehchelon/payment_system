use git_version::git_version;

pub const VERSION: &str = git_version!(args = ["--tags", "--abbrev=0"]); /*  increase when I want to compare which one I am running on prod 
just --tags shows some sort of hash at the end.
--abbrev being set to 0 is necessary to return only the tag.
*/
pub fn get() -> String {
    VERSION.to_string()
}