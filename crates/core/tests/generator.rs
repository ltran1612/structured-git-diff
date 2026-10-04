use structdiff_core::Generator;
use structdiff_core::generator::{expand, prompt};

#[test]
fn prompt_is_the_skill_without_front_matter_for_the_range() {
    let p = prompt("main...");
    assert!(p.starts_with("# diff-narrative"), "{}", &p[..40]);
    assert!(!p.contains("name: diff-narrative"));
    assert!(p.contains("Range argument: `main...`"));
    assert!(p.contains("structdiff export"));
    assert!(!p.contains("$ARGUMENTS"));
}

#[test]
fn each_generator_gets_the_prompt_and_narrow_permissions() {
    let out = ".structdiff/narrative-main....json";
    let claude = expand(&Generator::Claude.template(), "main...", out);
    assert_eq!(&claude[..2], ["claude", "-p"]);
    assert_eq!(claude[2], prompt("main..."));
    assert!(claude.contains(&"Bash(structdiff export:*)".to_owned()));

    let codex = expand(&Generator::Codex.template(), "main...", out);
    assert_eq!(&codex[..4], ["codex", "exec", "--sandbox", "workspace-write"]);
    assert_eq!(codex.last().unwrap(), &prompt("main..."));

    let copilot = expand(&Generator::Copilot.template(), "main...", out);
    assert_eq!(&copilot[..2], ["copilot", "-p"]);
    assert_eq!(copilot[2], prompt("main..."));
    assert_eq!(
        copilot.last().unwrap(),
        "--allow-tool=shell(git:*), shell(structdiff:*), write(.structdiff/narrative-main....json)"
    );
}

#[test]
fn custom_templates_fill_range_and_output() {
    let t: Vec<String> = ["my-agent", "/diff-narrative {range}", "--out={output}"].map(String::from).to_vec();
    assert_eq!(expand(&t, "", ".structdiff/narrative.json"), ["my-agent", "/diff-narrative", "--out=.structdiff/narrative.json"]);
    assert_eq!(expand(&t, "main...", "x"), ["my-agent", "/diff-narrative main...", "--out=x"]);
}

#[test]
fn generator_names_are_lowercase() {
    for (name, g) in [("claude", Generator::Claude), ("codex", Generator::Codex), ("copilot", Generator::Copilot)] {
        assert_eq!(serde_json::from_str::<Generator>(&format!("\"{name}\"")).unwrap(), g);
    }
    assert!(serde_json::from_str::<Generator>("\"gpt\"").is_err());
}
