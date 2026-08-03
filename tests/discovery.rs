mod common;

use gitcat::repo;

#[test]
fn lists_bare_repos_newest_first() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::bare_repo_with_commit(root, "older", "first commit", 1_000_000_000);
    common::bare_repo_with_commit(root, "newer", "second commit", 1_700_000_000);

    let repos = repo::discover(root).expect("discover");
    let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();

    assert_eq!(names, ["newer", "older"]);

    let head = repos[0].head.as_ref().expect("head commit");
    assert_eq!(head.summary, "second commit");
    assert_eq!(head.author, "Test Author");
    assert_eq!(head.seconds, 1_700_000_000);
    assert_eq!(head.id.len(), 7);
}

#[test]
fn empty_repos_sort_last_and_report_no_head() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::empty_bare_repo(root, "fresh");
    common::bare_repo_with_commit(root, "used", "a commit", 1_700_000_000);

    let repos = repo::discover(root).expect("discover");
    let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();

    assert_eq!(names, ["used", "fresh"]);
    assert!(repos[1].head.is_none());
}

#[test]
fn reads_description_but_ignores_the_git_default() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::empty_bare_repo(root, "described");
    common::empty_bare_repo(root, "plain");
    std::fs::write(
        root.join("described.git/description"),
        "A test repository\n",
    )
    .expect("write description");

    let repos = repo::discover(root).expect("discover");
    let described = repos.iter().find(|r| r.name == "described").expect("repo");
    let plain = repos.iter().find(|r| r.name == "plain").expect("repo");

    assert_eq!(described.description.as_deref(), Some("A test repository"));
    assert_eq!(plain.description, None);
}

#[test]
fn skips_entries_that_are_not_bare_repos() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::empty_bare_repo(root, "real");
    std::fs::create_dir(root.join("notarepo.git")).expect("create dir");
    std::fs::create_dir(root.join("plaindir")).expect("create dir");
    std::fs::write(root.join("afile.git"), "not a directory").expect("write file");

    let repos = repo::discover(root).expect("discover");
    let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();

    assert_eq!(names, ["real"]);
}

#[test]
fn opens_a_repo_by_name_with_or_without_suffix() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    common::bare_repo_with_commit(&root, "target", "a commit", 1_700_000_000);

    assert!(repo::open(&root, "target").is_ok());
    assert!(repo::open(&root, "target.git").is_ok());
    assert!(matches!(
        repo::open(&root, "missing"),
        Err(repo::RepoError::NotFound)
    ));
}

#[test]
fn refuses_to_open_outside_the_repo_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("repos");
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&root).expect("create root");
    std::fs::create_dir_all(&outside).expect("create outside");
    let root = root.canonicalize().expect("canonicalize");

    common::empty_bare_repo(&outside, "secret");

    #[cfg(unix)]
    std::os::unix::fs::symlink(outside.join("secret.git"), root.join("linked.git"))
        .expect("symlink");

    for name in ["../outside/secret", "..%2Foutside", "linked/../../outside"] {
        assert!(matches!(
            repo::open(&root, name),
            Err(repo::RepoError::InvalidName)
        ));
    }

    #[cfg(unix)]
    assert!(matches!(
        repo::open(&root, "linked"),
        Err(repo::RepoError::NotFound)
    ));
}

#[test]
fn discovers_working_repositories_alongside_bare_ones() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path();

    common::bare_repo_with_commit(root, "served", "bare commit", 1_700_000_000);

    let work = root.join("checkout");
    std::fs::create_dir_all(&work).expect("create work tree");
    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("a.txt"), "hello\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "working commit", 1_700_000_100);

    let repos = repo::discover(root).expect("discover");
    let names: Vec<_> = repos.iter().map(|r| r.name.as_str()).collect();

    assert_eq!(names, ["checkout", "served"]);

    let checkout = repos.iter().find(|r| r.name == "checkout").expect("repo");
    let served = repos.iter().find(|r| r.name == "served").expect("repo");

    assert!(!checkout.bare);
    assert!(served.bare);
    assert_eq!(
        checkout.head.as_ref().expect("head").summary,
        "working commit"
    );
}

#[test]
fn a_working_repository_opens_by_name() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    let work = root.join("checkout");
    std::fs::create_dir_all(&work).expect("create work tree");
    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("a.txt"), "hello\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "working commit", 1_700_000_000);

    assert!(repo::open(&root, "checkout").is_ok());
    assert!(matches!(
        repo::open(&root, "missing"),
        Err(repo::RepoError::NotFound)
    ));
}

#[test]
fn a_bare_repository_wins_a_name_collision() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    common::bare_repo_with_commit(&root, "both", "bare commit", 1_700_000_000);

    let work = root.join("both");
    std::fs::create_dir_all(&work).expect("create work tree");
    common::git(&work, &["init", "--initial-branch", "main", "."]);
    std::fs::write(work.join("a.txt"), "hello\n").expect("write");
    common::git(&work, &["add", "."]);
    common::commit(&work, "working commit", 1_700_000_100);

    let repos = repo::discover(&root).expect("discover");
    let listed: Vec<_> = repos.iter().filter(|r| r.name == "both").collect();

    assert_eq!(listed.len(), 1, "the name is listed once");
    assert!(listed[0].bare, "the bare repository is the one listed");
    assert_eq!(
        listed[0].head.as_ref().expect("head").summary,
        "bare commit"
    );

    let opened = repo::open(&root, "both").expect("open");
    assert!(
        opened.path().ends_with("both.git"),
        "opening resolves to the bare repository too"
    );
}

#[test]
fn a_plain_directory_is_not_a_repository() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().canonicalize().expect("canonicalize");

    std::fs::create_dir(root.join("notes")).expect("mkdir");
    std::fs::write(root.join("notes/todo.txt"), "things\n").expect("write");

    assert!(repo::discover(&root).expect("discover").is_empty());
    assert!(matches!(
        repo::open(&root, "notes"),
        Err(repo::RepoError::NotFound)
    ));
}
