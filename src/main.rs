extern crate reqwest;
extern crate select;
extern crate slack_hook;

use slack_hook::{AttachmentBuilder, Field, PayloadBuilder, Slack};
use std::io::Read;
use std::collections::{HashMap, HashSet};
use select::document::Document;
use select::predicate::{Child, Class, Name, Or};
use std::env;

struct Dish<'a> {
    diets: HashSet<&'a str>,
    allergens: Option<HashSet<&'a str>>,
    icon: &'static str,
}

fn main() {
    let slack = Slack::new(&*env::var("SLACK_WEBHOOK_URL").unwrap()).unwrap();
    let mut resp = reqwest::get(
        "https://mit.campusdish.com/Commerce/Catalog/Menus.aspx?LocationId=9333&PeriodId=1440",
    ).unwrap();
    assert!(resp.status().is_success());

    let mut content = String::new();
    resp.read_to_string(&mut content).unwrap();

    let document = Document::from(&*content);
    let mut dishes: HashMap<String, Dish> = HashMap::new();
    for station in document.find(Class("menu-details-station")) {
        let name = station.find(Name("h2")).next().unwrap();
        let name = name.text();

        match &*name {
            "Calzones" | "Hot Food" | "Pasta" | "World Flavors" => {}
            _ => continue,
        }

        let mut category = "".to_string();
        'item: for element in
            station.find(Or(Class("category"), Class("menu-details-station-item")))
        {
            let class = element.attr("class").unwrap();
            let classes = class.split_whitespace();

            let mut diets = HashSet::new();
            let mut allergens = HashSet::new();
            let mut unknown_allergens = HashSet::new();
            for class in classes {
                match class {
                    "category" => {
                        category = element.text();
                        continue 'item;
                    }
                    c if c.starts_with("d") => {
                        diets.insert(c.trim_left_matches('d'));
                    }
                    c if c.starts_with("au") => {
                        unknown_allergens.insert(c.trim_left_matches("au"));
                    }
                    c if c.starts_with("a") => {
                        allergens.insert(c.trim_left_matches('a'));
                    }
                    _ => {}
                }
            }

            let icon = match (&*name, &*category) {
                ("Pasta", "Pasta Entrées") => Some(":spaghetti:"),
                ("Pasta", "Vegetable Entrees") => Some(":stew:"),
                ("Hot Food", "Sides") => Some(":rice:"),
                ("World Flavors", "Sides") => Some(":rice:"),
                ("World Flavors", _) => None,
                ("Hot Food", _) => None,
                _ => continue,
            };

            let icon = icon.unwrap_or_else(|| {
                // let's play "guess the icon"
                if diets.contains("Vegan") {
                    // if it's vegan, it's vegan.
                    ":seedling:"
                } else if diets.contains("Vegetarian") {
                    // if it's vegetarian, it's not meat
                    ":tomato:"
                } else if allergens.contains("Fish") {
                    // it's probably fish
                    ":fish:"
                } else if allergens.contains("Shellfish") {
                    ":crab:"
                } else {
                    // meat I guess?
                    ":poultry_leg:"
                }
            });

            let dish = element
                .find(Child(
                    Class("menu-name"),
                    Or(Class("menu-item-name"), Name("a")),
                ))
                .next()
                .unwrap()
                .text();

            let allergens = if unknown_allergens.is_empty() {
                Some(allergens)
            } else {
                None
            };

            if let Some(d) = dishes.get(&dish) {
                if d.icon != ":rice:" {
                    // keep current non-side version
                    continue;
                }
            }

            dishes.insert(
                dish,
                Dish {
                    diets,
                    allergens,
                    icon,
                },
            );
        }
    }

    let mut meats = Vec::new();
    let mut veggies = Vec::new();
    let mut sides = Vec::new();
    for (
        dish,
        Dish {
            mut diets,
            allergens,
            icon,
        },
    ) in dishes
    {
        let veggie = diets.contains("Vegetarian");
        if diets.contains("Vegan") {
            // redundant
            diets.remove("Vegetarian");
        }

        let mut is = Vec::new();
        for diet in diets {
            is.push(match &*diet {
                "Vegan" => "vegan",
                "Vegetarian" => "vegetarian",
                "Kosher" => "kosher",
                "Gluten" => "gluten-free",
                _ => unimplemented!(),
            });
        }

        let has: Option<Vec<_>> = allergens.map(|allergens| {
            allergens
                .into_iter()
                .map(|allergen| match &*allergen {
                    "Wheat" => "wheat",
                    "Soy" => "soy",
                    "Milk" => "milk",
                    "Peanuts" => "peanuts",
                    "TreeNuts" => "treenuts",
                    "Shellfish" => "shellfish",
                    "Fish" => "fish",
                    "Eggs" => "eggs",
                    _ => unimplemented!(),
                })
                .collect()
        });

        let is = match is.len() {
            0 => "".to_string(),
            1 => format!("{}", is[0]),
            2 => format!("{} and {}", is[0], is[1]),
            n => format!("{}", is.join(", and ")).replacen(", and ", ", ", n.saturating_sub(2)),
        };
        let is = is.trim();

        let mut has_str = String::new();
        if !is.is_empty() && (has.is_none() || !has.as_ref().unwrap().is_empty()) {
            has_str.push_str("; ");
        }

        has_str.push_str(&*match has.as_ref().map(|h| h.len()) {
            None => "allergens unknown".to_string(),
            Some(n) => {
                let has = has.unwrap();
                match n {
                    0 => "".to_string(),
                    1 => format!("contains {}", has[0]),
                    2 => format!("contains {} and {}", has[0], has[1]),
                    n => format!("contains {}", has.join(", and ")).replacen(
                        ", and ",
                        ", ",
                        n.saturating_sub(2),
                    ),
                }
            }
        });

        let field = Field::new(dish, format!("{} {}{}", icon, is, has_str), Some(true));
        if icon == ":rice:" {
            sides.push(field);
        } else {
            if veggie {
                veggies.push(field);
            } else {
                meats.push(field);
            }
        }
    }

    let p = PayloadBuilder::new()
        .text("Today's menu")
        .attachments(vec![
            AttachmentBuilder::new("Meat entrées")
                .text("")
                .fields(meats)
                .color("danger")
                .build()
                .unwrap(),
            AttachmentBuilder::new("Vegetarian entrées")
                .text("")
                .fields(veggies)
                .color("good")
                .build()
                .unwrap(),
            AttachmentBuilder::new("Sides")
                .text("")
                .fields(sides)
                .build()
                .unwrap(),
        ])
        .build()
        .unwrap();

    slack.send(&p).unwrap();
}
