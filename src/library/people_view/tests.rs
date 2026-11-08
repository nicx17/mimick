use super::*;

fn person(id: &str, name: &str, hidden: bool) -> Person {
    Person {
        id: id.into(),
        name: name.into(),
        is_hidden: hidden,
        birth_date: None,
        is_favorite: false,
    }
}

#[test]
fn visible_people_filters_hidden_and_search() {
    let people = vec![
        person("1", "Ana", false),
        person("2", "Ben", true),
        person("3", "", false),
    ];
    let ids = |shown: Vec<&Person>| shown.iter().map(|p| p.id.clone()).collect::<Vec<_>>();
    assert_eq!(ids(visible_people(&people, "", false)), ["1", "3"]);
    assert_eq!(ids(visible_people(&people, "", true)), ["1", "2", "3"]);
    assert_eq!(ids(visible_people(&people, "be", true)), ["2"]);
}

#[test]
fn favorites_come_first_in_their_original_order() {
    let mut people = vec![
        person("1", "Ana", false),
        person("2", "Ben", false),
        person("3", "Cy", false),
    ];
    people[1].is_favorite = true;
    people[2].is_favorite = true;
    let ids: Vec<&str> = visible_people(&people, "", false)
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(ids, ["2", "3", "1"]);
}

#[test]
fn apply_update_changes_only_set_fields() {
    let mut ana = person("1", "Ana", false);
    apply_update(
        &mut ana,
        &PersonUpdate {
            is_hidden: Some(true),
            birth_date: Some(Some("1990-05-04".into())),
            ..PersonUpdate::default()
        },
    );
    assert_eq!(ana.name, "Ana");
    assert!(ana.is_hidden);
    assert_eq!(ana.birth_date.as_deref(), Some("1990-05-04"));
}

#[test]
fn parse_date_reads_iso_dates() {
    assert_eq!(parse_date("1990-05-04"), Some((1990, 5, 4)));
    assert_eq!(parse_date("1990-05-04T00:00:00.000Z"), Some((1990, 5, 4)));
    assert_eq!(parse_date("bad"), None);
}
