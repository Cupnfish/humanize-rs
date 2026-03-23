use super::planning::PlanningStore;
use super::*;

pub(super) fn handle_gen_draft(
    input: Option<&str>,
    title: Option<&str>,
    stdin: bool,
) -> Result<()> {
    let content = read_draft_content(input, stdin)?;
    if content.trim().is_empty() {
        bail!("Draft content is empty.");
    }

    let project_root = resolve_project_root()?;
    let mut store = PlanningStore::load(&project_root)?;
    let draft = store.create_draft(&content, title)?;
    let relative_path = store.project_relative_path(&draft.path)?;

    println!("Draft Handle: {}", draft.handle);
    println!("Thread: {}", draft.thread_id);
    println!("Draft Path: {}", relative_path);
    Ok(())
}

fn read_draft_content(input: Option<&str>, stdin: bool) -> Result<String> {
    match (input, stdin) {
        (Some(_), true) => bail!("Cannot use --input and --stdin together."),
        (Some(path), false) => {
            let input_path = PathBuf::from(path);
            if !input_path.is_file() {
                bail!("Input file not found: {}", input_path.display());
            }
            Ok(fs::read_to_string(&input_path)?)
        }
        (None, true) | (None, false) if !std::io::stdin().is_terminal() => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
        (None, true) => {
            let mut buf = String::new();
            std::io::stdin().read_to_string(&mut buf)?;
            Ok(buf)
        }
        (None, false) => bail!("gen-draft requires --input <path> or --stdin."),
    }
}
