use anyhow::{anyhow, ensure, Context, Result};
use scraper::{node::Element, CaseSensitivity, ElementRef, Node, Selector};

use crate::{Action as ActionScores, Parsed, Round, Turn};

type NodeRef<'a> = ego_tree::NodeRef<'a, Node>;

pub struct Parser {
    round_heading: Selector,
    round_heading_to_turn: Selector,
    turn_to_role: Selector,
    turn_to_action: Selector,
}

impl Parser {
    pub fn new() -> Self {
        fn selector(s: &str) -> Selector {
            Selector::parse(s).unwrap()
        }

        Self {
            round_heading: selector("html > body > section > h1.kyoku-heading"),
            round_heading_to_turn: selector("div:nth-child(4) > details:nth-child(2)"),
            turn_to_role: selector("span.role"),
            turn_to_action: selector("details > table > tbody > tr"),
        }
    }

    pub fn parse(&self, file: &str) -> Result<Parsed> {
        let html = scraper::html::Html::parse_document(file);
        let rounds = html
            .select(&self.round_heading)
            .map(|a| self.parse_round(a))
            .collect::<Result<_>>()
            .context("parse round")?;
        Ok(Parsed { rounds })
    }

    fn parse_round(&self, round_heading: ElementRef) -> Result<Round> {
        let _name = round_heading
            .value()
            .id()
            .context("missing round heading id")?;
        let parent = round_heading
            .parent()
            .context("missing round heading parent")?;
        let parent = ElementRef::wrap(parent).context("wrap round heading parent")?;

        let turns = parent
            .select(&self.round_heading_to_turn)
            .map(|a| self.parse_turn(a))
            .collect::<Result<_>>()
            .context("parse turn")?;
        Ok(Round { turns })
    }

    fn parse_turn(&self, turn: ElementRef) -> Result<Turn> {
        let mut roles = turn.select(&self.turn_to_role);
        let player = roles.next().context("missing player role in turn")?;
        let mortal = roles.next().context("missing mortal role in turn")?;
        ensure!(roles.next().is_none(), "unexpected third role in turn");
        let player = self
            .parse_role(player, "Player: ")
            .context("parse role player")?;
        let mortal = self
            .parse_role(mortal, "Mortal: ")
            .context("parse role mortal")?;

        let actions = turn
            .select(&self.turn_to_action)
            .map(|a| self.parse_action_with_scores(a))
            .collect::<Result<Vec<_>>>()
            .context("parse action with scores")?;
        let find_action_index = |action: &Action| -> Result<usize> {
            actions
                .iter()
                .position(|action_| *action == action_.0)
                .context("action not found")
        };
        Ok(Turn {
            player: find_action_index(&player)?,
            mortal: find_action_index(&mortal)?,
            actions: actions.into_iter().map(|action| action.1).collect(),
        })
    }

    fn parse_role<'a>(&self, role: ElementRef<'a>, expected_role_name: &str) -> Result<Action<'a>> {
        let role_name: &str = role
            .first_child()
            .context("no child")?
            .value()
            .as_text()
            .context("child is not text")?
            .as_ref();
        ensure!(role_name == expected_role_name, "unexpected role name");
        self.parse_action(role.next_siblings().take_while(|node| {
            let Node::Element(element) = node.value() else {
                return true;
            };
            element.name() != "details"
        }))
        .context("parse action")
    }

    fn parse_action_with_scores<'a>(
        &self,
        parent: ElementRef<'a>,
    ) -> Result<(Action<'a>, ActionScores)> {
        let mut children = parent.children().filter(|child| child.value().is_element());
        let action = children.next().context("no first child")?;
        let q = children.next().context("no second child")?;
        let pi = children.next().context("no third child")?;
        ensure!(children.next().is_none(), "unexpected more children");

        let action = self
            .parse_action(action.children())
            .context("parse action")?;
        let q = self.parse_action_score(q).context("parse action score")?;
        let pi = self.parse_action_score(pi).context("parse action score")?;

        Ok((action, ActionScores { q, pi }))
    }

    fn parse_action_score(&self, parent: NodeRef) -> Result<f32> {
        let mut children = parent.children();
        let first = children.next().context("no child")?;
        let second = children.next().context("no child")?;

        let int = self
            .parse_action_score_part(first, "int")
            .context("parse action score int")?;
        ensure!(int.ends_with('.'), "integer part doesn't end with dot");
        let frac = self
            .parse_action_score_part(second, "frac")
            .context("parse action score frac")?;

        let combined = format!("{}{}", int, frac);
        combined.parse().context("parse f32 from {combined:?}")
    }

    fn parse_action_score_part<'a>(
        &self,
        node: NodeRef<'a>,
        expected_class: &str,
    ) -> Result<&'a str> {
        let Node::Element(element) = node.value() else {
            return Err(anyhow!("node is not element"));
        };
        ensure!(element.name() == "span", "element is not span");
        ensure!(
            element.has_class(expected_class, CaseSensitivity::CaseSensitive),
            "missing expected class"
        );
        let mut children = node.children();
        let first = children.next().context("no children")?;
        ensure!(children.next().is_none(), "unexpected more children");
        first
            .value()
            .as_text()
            .map(|text| text.as_ref())
            .context("child is not text")
    }

    fn parse_action<'a>(&self, nodes: impl Iterator<Item = NodeRef<'a>>) -> Result<Action<'a>> {
        let action_elements: Vec<ActionElement> = nodes
            .filter_map(|child| match child.value() {
                Node::Text(text) => {
                    let text = text.trim();
                    if text.is_empty() {
                        return None;
                    }
                    Some(Ok(ActionElement::Text(text)))
                }
                Node::Element(element) if element.name() == "svg" => Some(
                    self.parse_svg_action_element(&child, element)
                        .context("parse svg action element")
                        .map(ActionElement::Tile),
                ),
                _ => None,
            })
            .collect::<Result<_>>()?;
        ensure!(!action_elements.is_empty(), "empty action");
        Ok(Action(action_elements))
    }

    fn parse_svg_action_element<'a>(
        &self,
        node: &NodeRef<'a>,
        element: &'a Element,
    ) -> Result<&'a str> {
        ensure!(element.name() == "svg", "element name is not svg");
        ensure!(
            element.has_class("tile", CaseSensitivity::CaseSensitive),
            "element class is not tile",
        );
        let child = node
            .children()
            .find_map(|child| child.value().as_element())
            .context("no child")?;
        ensure!(child.name() == "use", "element name is not use");
        ensure!(
            child.has_class("face", CaseSensitivity::CaseSensitive),
            "element class is not face"
        );
        child.attr("href").context("no href attribute")
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Action<'a>(Vec<ActionElement<'a>>);

#[derive(Debug, Eq, PartialEq)]
enum ActionElement<'a> {
    Text(&'a str),
    Tile(&'a str),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test0() {
        let _parsed = Parser::new().parse(include_str!("../test0.html")).unwrap();
        // println!("{parsed:#?}");
    }

    #[test]
    fn test1() {
        let _parsed = Parser::new().parse(include_str!("../test1.html")).unwrap();
        // println!("{parsed:#?}");
    }
}
