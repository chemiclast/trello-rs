use crate::find;
use clap::ArgMatches;
use colored::*;
use std::env;
use std::error::Error;
use std::io::{Read, Write};
use std::process;
use std::{thread, time};
use tempfile::Builder;
use trello::{
    search, Attachment, Board, Card, CardContents, Client, Label, List, Renderable, TrelloError,
};

fn get_input(text: &str) -> Result<String, rustyline::error::ReadlineError> {
    let mut rl = rustyline::Editor::<()>::new();
    rl.bind_sequence(
        rustyline::KeyPress::ControlLeft,
        rustyline::Cmd::Move(rustyline::Movement::BackwardWord(1, rustyline::Word::Big)),
    );
    rl.bind_sequence(
        rustyline::KeyPress::ControlRight,
        rustyline::Cmd::Move(rustyline::Movement::ForwardWord(
            1,
            rustyline::At::Start,
            rustyline::Word::Big,
        )),
    );
    rl.readline(text)
}

/// Opens the users chosen editor (specified by the $EDITOR environment variable)
/// to edit a specified card. If $EDITOR is not set, the default editor will fallback
/// to vi.
///
/// This function will upload any changes written by the editor to Trello. This includes
/// when the editor is not closed but content is saved.
fn edit_card(client: &Client, card: &Card) -> Result<(), Box<dyn Error>> {
    let mut file = Builder::new().suffix(".md").tempfile()?;
    let editor_env = env::var("EDITOR").unwrap_or_else(|_| String::from("vi"));

    debug!("Using editor: {}", editor_env);
    debug!("Editing card: {:?}", card);

    writeln!(file, "{}", card.render())?;

    let mut new_card = card.clone();

    // Outer retry loop - reopen editor if last upload attempt failed
    loop {
        let mut editor = process::Command::new(&editor_env)
            .arg(file.path())
            .spawn()?;
        let mut result: Option<Result<Card, TrelloError>> = None;

        // Inner watch loop - look out for card changes to upload
        loop {
            const SLEEP_TIME: u64 = 500;
            debug!("Sleeping for {}ms", SLEEP_TIME);
            thread::sleep(time::Duration::from_millis(SLEEP_TIME));

            let mut buf = String::new();
            file.reopen()?.read_to_string(&mut buf)?;

            // Trim end because a lot of editors will use auto add new lines at the end of the file
            let contents: CardContents = match buf.trim_end().parse() {
                Ok(c) => c,
                Err(e) => {
                    debug!("Unable to parse Card Contents: {}", e);
                    continue;
                }
            };

            // if no upload attempts
            // if previous loop had a failure
            // if card in memory is different to card in file
            if result.is_none()
                || result.as_ref().unwrap().is_err()
                || new_card.name != contents.name
                || new_card.desc != contents.desc
            {
                new_card.name = contents.name;
                new_card.desc = contents.desc;

                debug!("Updating card: {:?}", new_card);
                result = Some(Card::update(client, &new_card));

                match &result {
                    Some(Ok(_)) => debug!("Updated card"),
                    Some(Err(e)) => debug!("Error updating card {:?}", e),
                    None => panic!("This should be impossible"),
                };
            }

            if let Some(ecode) = editor.try_wait()? {
                debug!("Exiting editor loop with code: {}", ecode);
                break;
            }
        }

        match &result {
            None => {
                debug!("Exiting retry loop due to no result being ever retrieved");
                break;
            }
            Some(Ok(_)) => {
                debug!("Exiting retry loop due to successful last update");
                break;
            }
            Some(Err(e)) => {
                eprintln!("An error occurred while trying to update the card.");
                eprintln!("{}", e);
                eprintln!();
                get_input("Press entry to re-enter editor")?;
            }
        }
    }

    Ok(())
}

pub fn show_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running show subcommand with {:?}", matches);

    let label_filter = matches.value_of("label_filter");

    let params = find::get_trello_params(matches);
    debug!("Trello Params: {:?}", params);

    let result = find::get_trello_object(client, &params)?;
    trace!("result: {:?}", result);

    if let Some(card) = result.card {
        edit_card(client, &card)?;
    } else if let Some(list) = result.list {
        let list = match label_filter {
            Some(label_filter) => list.filter(label_filter),
            None => list,
        };
        println!("{}", list.render());
    } else if let Some(mut board) = result.board {
        board.retrieve_nested(client)?;
        let board = match label_filter {
            Some(label_filter) => board.filter(label_filter),
            None => board,
        };

        println!("{}", board.render());
    } else {
        println!("Open Boards");
        println!("===========");
        println!();

        let boards = Board::get_all(client)?;
        for b in boards {
            println!("* {}", b.name);
        }
    }

    Ok(())
}

pub fn open_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running open subcommand with {:?}", matches);

    let id = matches.value_of("id").ok_or("Id not provided")?;
    let object_type = matches.value_of("type").ok_or("type not provided")?;

    if object_type == "board" {
        debug!("Re-opening board with id {}", &id);
        let board = Board::open(client, &id)?;

        eprintln!("Opened board: {}", &board.name.green());
        eprintln!("id: {}", &board.id);
    } else if object_type == "list" {
        debug!("Re-opening list with id {}", &id);
        let list = List::open(client, &id)?;

        eprintln!("Opened list: {}", &list.name.green());
        eprintln!("id: {}", &list.id);
    } else if object_type == "card" {
        debug!("Re-openning card with id {}", &id);
        let card = Card::open(client, &id)?;

        eprintln!("Opened card: {}", &card.name.green());
        eprintln!("id: {}", &card.id);
    } else {
        bail!("Unknown object_type {}", object_type);
    }

    Ok(())
}

pub fn close_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running close subcommand with {:?}", matches);

    let params = find::get_trello_params(matches);
    let result = find::get_trello_object(client, &params)?;

    let show = matches.is_present("show");

    trace!("result: {:?}", result);

    if let Some(mut card) = result.card {
        card.closed = true;
        Card::update(client, &card)?;

        // FIXME: Bug shows the board with closed card
        if show {
            println!("{}", result.board.unwrap().render());
            println!();
        }

        eprintln!("Closed card: '{}'", &card.name.green());
        eprintln!("id: {}", &card.id);
    } else if let Some(mut list) = result.list {
        list.closed = true;
        List::update(client, &list)?;

        // FIXME: Bug shows the board with the closed list
        if show {
            println!("{}", result.board.unwrap().render());
            println!();
        }

        eprintln!("Closed list: '{}'", &list.name.green());
        eprintln!("id: {}", &list.id);
    } else if let Some(mut board) = result.board {
        board.closed = true;
        Board::update(client, &board)?;
        eprintln!("Closed board: '{}'", &board.name.green());
        eprintln!("id: {}", &board.id);
    }

    Ok(())
}

pub fn create_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running create subcommand with {:?}", matches);

    let params = find::get_trello_params(matches);
    let result = find::get_trello_object(client, &params)?;

    let show = matches.is_present("show");

    trace!("result: {:?}", result);

    if let Some(list) = result.list {
        let name = get_input("Card name: ")?;

        let card = Card::create(client, &list.id, &Card::new("", &name, "", None, ""))?;

        if show {
            edit_card(client, &card)?;
        }
    } else if let Some(board) = result.board {
        let name = get_input("List name: ")?;

        List::create(client, &board.id, &name)?;
    } else {
        let name = get_input("Board name: ")?;

        Board::create(client, &name)?;
    }

    Ok(())
}
pub fn attachments_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running attachments subcommand with {:?}", matches);

    let params = find::get_trello_params(matches);
    let result = find::get_trello_object(client, &params)?;

    let card = result.card.ok_or("Unable to find card")?;

    let attachments = Attachment::get_all(client, &card.id)?;

    for att in attachments {
        println!("{}", &att.url);
    }

    Ok(())
}

pub fn attach_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running attach subcommand with {:?}", matches);

    let params = find::get_trello_params(matches);
    let result = find::get_trello_object(client, &params)?;

    let path = matches.value_of("path").unwrap();

    let card = result.card.ok_or("Unable to find card")?;

    let attachment = Attachment::apply(client, &card.id, path)?;

    println!("{}", attachment.render());

    Ok(())
}

pub fn url_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running url subcommand with {:?}", matches);

    let params = find::get_trello_params(matches);
    let result = find::get_trello_object(client, &params)?;

    if let Some(card) = result.card {
        println!("{}", card.url);
    } else if result.list.is_some() {
        // Lists do not have a target url
        // We can display the parent board url instead
        println!("{}", result.board.unwrap().url);
    } else if let Some(board) = result.board {
        println!("{}", board.url);
    }
    Ok(())
}

pub fn search_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running search subcommand with {:?}", matches);

    let query = matches.value_of("query").ok_or("Missing query value")?;
    let partial = matches.is_present("partial");

    let results = search(client, &query, partial)?;

    if !&results.cards.is_empty() {
        println!("Cards");
        println!("-----");

        for card in &results.cards {
            let card_state = match card.closed {
                true => "[Closed]".red().to_string(),
                false => "".to_string(),
            };
            println!("'{}' id: {} {}", card.name.green(), card.id, card_state);
        }
        println!();
    }

    if !&results.boards.is_empty() {
        println!("Boards");
        println!("------");

        for board in &results.boards {
            println!("'{}' id: {}", board.name.green(), board.id);
        }
        println!();
    }

    Ok(())
}

pub fn label_subcommand(client: &Client, matches: &ArgMatches) -> Result<(), Box<dyn Error>> {
    debug!("Running label subcommand with {:?}", matches);

    let params = find::get_trello_params(matches);
    let result = find::get_trello_object(client, &params)?;

    let labels = Label::get_all(&client, &result.board.ok_or("Unable to retrieve board")?.id)?;
    let card = result.card.ok_or("Unable to find card")?;

    let label_name = matches.value_of("label_name").unwrap();
    let delete = matches.is_present("delete");

    let label = find::get_object_by_name(&labels, label_name, params.ignore_case)?;
    let card_has_label = card
        .labels
        .ok_or("Unable to retrieve Card labels")?
        .iter()
        .any(|l| l.id == label.id);

    if delete {
        if !card_has_label {
            eprintln!(
                "Label [{}] does not exist on '{}'",
                &label.colored_name(),
                &card.name.green(),
            );
        } else {
            Label::remove(client, &card.id, &label.id)?;

            eprintln!(
                "Removed [{}] label from '{}'",
                &label.colored_name(),
                &card.name.green(),
            );
        }
    } else if card_has_label {
        eprintln!(
            "Label [{}] already exists on '{}'",
            &label.colored_name(),
            &card.name.green()
        );
    } else {
        Label::apply(client, &card.id, &label.id)?;

        eprintln!(
            "Applied [{}] label to '{}'",
            &label.colored_name(),
            &card.name.green()
        );
    }

    Ok(())
}
