use std::collections::{BTreeMap, BTreeSet};

use crate::{builtin::builtin_imports::*, serializer::inspect_number, value::fuzzy_round};

pub(crate) fn function_string(
    name: &'static str,
    args: &[Value],
    visitor: &mut Visitor,
    span: Span,
) -> SassResult<String> {
    let args = args
        .iter()
        .map(|arg| arg.to_css_string(span, visitor.parser.options.is_compressed()))
        .collect::<SassResult<Vec<_>>>()?
        .join(", ");

    Ok(format!("{}({})", name, args))
}

fn inner_rgb_2_arg(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    // rgba(var(--foo), 0.5) is valid CSS because --foo might be `123, 456, 789`
    // and functions are parsed after variable substitution.
    let color = args.get_err(0, "color")?;
    let alpha = args.get_err(1, "alpha")?;

    let is_compressed = visitor.parser.options.is_compressed();

    if color.is_var() {
        return Ok(Value::String(
            function_string(name, &[color, alpha], visitor, args.span())?,
            QuoteKind::None,
        ));
    } else if alpha.is_var() {
        match &color {
            Value::Color(color) => {
                return Ok(Value::String(
                    format!(
                        "{}({}, {}, {}, {})",
                        name,
                        color.red().to_string(is_compressed),
                        color.green().to_string(is_compressed),
                        color.blue().to_string(is_compressed),
                        alpha.to_css_string(args.span(), is_compressed)?
                    ),
                    QuoteKind::None,
                ));
            }
            _ => {
                return Ok(Value::String(
                    function_string(name, &[color, alpha], visitor, args.span())?,
                    QuoteKind::None,
                ))
            }
        }
    } else if alpha.is_special_function() {
        let color = color.assert_color_with_name("color", args.span())?;

        return Ok(Value::String(
            format!(
                "{}({}, {}, {}, {})",
                name,
                color.red().to_string(is_compressed),
                color.green().to_string(is_compressed),
                color.blue().to_string(is_compressed),
                alpha.to_css_string(args.span(), is_compressed)?
            ),
            QuoteKind::None,
        ));
    }

    let color = color.assert_color_with_name("color", args.span())?;
    let alpha = alpha.assert_number_with_name("alpha", args.span())?;
    Ok(Value::Color(Box::new(color.with_alpha(Number(
        percentage_or_unitless(&alpha, 1.0, "alpha", args.span(), visitor)?,
    )))))
}

fn inner_rgb_3_arg(
    name: &'static str,
    mut args: ArgumentResult,
    visitor: &mut Visitor,
) -> SassResult<Value> {
    let alpha = if args.len() > 3 {
        args.get(3, "alpha")
    } else {
        None
    };

    let red = args.get_err(0, "red")?;
    let green = args.get_err(1, "green")?;
    let blue = args.get_err(2, "blue")?;

    if red.is_special_function()
        || green.is_special_function()
        || blue.is_special_function()
        || alpha
            .as_ref()
            .map(|alpha| alpha.node.is_special_function())
            .unwrap_or(false)
    {
        let fn_string = if alpha.is_some() {
            function_string(
                name,
                &[red, green, blue, alpha.unwrap().node],
                visitor,
                args.span(),
            )?
        } else {
            function_string(name, &[red, green, blue], visitor, args.span())?
        };

        return Ok(Value::String(fn_string, QuoteKind::None));
    }

    let span = args.span();

    let red = red.assert_number_with_name("red", span)?;
    let green = green.assert_number_with_name("green", span)?;
    let blue = blue.assert_number_with_name("blue", span)?;

    Ok(Value::Color(Box::new(Color::from_rgba_fn(
        Number(fuzzy_round(percentage_or_unitless(
            &red, 255.0, "red", span, visitor,
        )?)),
        Number(fuzzy_round(percentage_or_unitless(
            &green, 255.0, "green", span, visitor,
        )?)),
        Number(fuzzy_round(percentage_or_unitless(
            &blue, 255.0, "blue", span, visitor,
        )?)),
        Number(
            alpha
                .map(|alpha| {
                    percentage_or_unitless(
                        &alpha.node.assert_number_with_name("alpha", span)?,
                        1.0,
                        "alpha",
                        span,
                        visitor,
                    )
                })
                .transpose()?
                .unwrap_or(1.0),
        ),
    ))))
}

pub(crate) fn percentage_or_unitless(
    number: &SassNumber,
    max: f64,
    name: &str,
    span: Span,
    visitor: &mut Visitor,
) -> SassResult<f64> {
    let value = if number.unit == Unit::None {
        number.num
    } else if number.unit == Unit::Percent {
        (number.num * max) / 100.0
    } else {
        return Err((
            format!(
                "${name}: Expected {} to have no units or \"%\".",
                inspect_number(&number, visitor.parser.options, span)?
            ),
            span,
        )
            .into());
    };

    Ok(0.0_f64.max(value.min(max)))
}

#[derive(Debug, Clone)]
pub(crate) enum ParsedChannels {
    String(String),
    List(Vec<Value>),
}

fn is_var_slash(value: &Value) -> bool {
    match value {
        Value::String(text, QuoteKind::Quoted) => {
            text.to_ascii_lowercase().starts_with("var(") && text.contains('/')
        }
        _ => false,
    }
}

pub(crate) fn parse_channels(
    name: &'static str,
    arg_names: &[&'static str],
    mut channels: Value,
    visitor: &mut Visitor,
    span: Span,
) -> SassResult<ParsedChannels> {
    if channels.is_var() {
        let fn_string = function_string(name, &[channels], visitor, span)?;
        return Ok(ParsedChannels::String(fn_string));
    }

    let original_channels = channels.clone();

    let mut alpha_from_slash_list = None;

    if channels.separator() == ListSeparator::Slash {
        let list = channels.clone().as_list();
        if list.len() != 2 {
            return Err((
                format!(
                    "Only 2 slash-separated elements allowed, but {} {} passed.",
                    list.len(),
                    if list.len() == 1 { "was" } else { "were" }
                ),
                span,
            )
                .into());
        }

        channels = list[0].clone();
        let inner_alpha_from_slash_list = list[1].clone();

        if !alpha_from_slash_list
            .as_ref()
            .map(Value::is_special_function)
            .unwrap_or(false)
        {
            inner_alpha_from_slash_list
                .clone()
                .assert_number_with_name("alpha", span)?;
        }

        alpha_from_slash_list = Some(inner_alpha_from_slash_list);

        if list[0].is_var() {
            let fn_string = function_string(name, &[original_channels], visitor, span)?;
            return Ok(ParsedChannels::String(fn_string));
        }
    }

    let is_comma_separated = channels.separator() == ListSeparator::Comma;
    let is_bracketed = matches!(channels, Value::List(_, _, Brackets::Bracketed));

    if is_comma_separated || is_bracketed {
        let mut err_buffer = "$channels must be".to_owned();

        if is_bracketed {
            err_buffer.push_str(" an unbracketed");
        }

        if is_comma_separated {
            if is_bracketed {
                err_buffer.push(',');
            } else {
                err_buffer.push_str(" a");
            }

            err_buffer.push_str(" space-separated");
        }

        err_buffer.push_str(" list.");

        return Err((err_buffer, span).into());
    }

    let mut list = channels.clone().as_list();

    if list.len() > 3 {
        return Err((
            format!("Only 3 elements allowed, but {} were passed.", list.len()),
            span,
        )
            .into());
    } else if list.len() < 3 {
        if list.iter().any(Value::is_var)
            || (!list.is_empty() && is_var_slash(list.last().unwrap()))
        {
            let fn_string = function_string(name, &[original_channels], visitor, span)?;
            return Ok(ParsedChannels::String(fn_string));
        } else {
            let argument = arg_names[list.len()];
            return Err((format!("Missing element ${argument}."), span).into());
        }
    }

    if let Some(alpha_from_slash_list) = alpha_from_slash_list {
        list.push(alpha_from_slash_list);
        return Ok(ParsedChannels::List(list));
    }

    match &list[2] {
        Value::Dimension { as_slash, .. } => match as_slash {
            Some(slash) => {
                // todo: superfluous clone
                let slash_0 = slash.0.clone();
                let slash_1 = slash.1.clone();
                Ok(ParsedChannels::List(vec![
                    list[0].clone(),
                    list[1].clone(),
                    Value::Dimension {
                        num: Number(slash_0.num),
                        unit: slash_0.unit,
                        as_slash: slash_0.as_slash,
                    },
                    Value::Dimension {
                        num: Number(slash_1.num),
                        unit: slash_1.unit,
                        as_slash: slash_1.as_slash,
                    },
                ]))
            }
            None => Ok(ParsedChannels::List(list)),
        },
        Value::String(text, QuoteKind::None) if text.contains('/') => {
            let fn_string = function_string(name, &[channels], visitor, span)?;
            Ok(ParsedChannels::String(fn_string))
        }
        _ => Ok(ParsedChannels::List(list)),
    }
}

/// name: Either `rgb` or `rgba` depending on the caller
fn inner_rgb(
    name: &'static str,
    mut args: ArgumentResult,
    parser: &mut Visitor,
) -> SassResult<Value> {
    args.max_args(4)?;

    match args.len() {
        0 | 1 => {
            match parse_channels(
                name,
                &["red", "green", "blue"],
                args.get_err(0, "channels")?,
                parser,
                args.span(),
            )? {
                ParsedChannels::String(s) => Ok(Value::String(s, QuoteKind::None)),
                ParsedChannels::List(list) => {
                    let args = ArgumentResult {
                        positional: list,
                        named: BTreeMap::new(),
                        separator: ListSeparator::Comma,
                        span: args.span(),
                        touched: BTreeSet::new(),
                    };

                    inner_rgb_3_arg(name, args, parser)
                }
            }
        }
        2 => inner_rgb_2_arg(name, args, parser),
        _ => inner_rgb_3_arg(name, args, parser),
    }
}

pub(crate) fn rgb(args: ArgumentResult, parser: &mut Visitor) -> SassResult<Value> {
    inner_rgb("rgb", args, parser)
}

pub(crate) fn rgba(args: ArgumentResult, parser: &mut Visitor) -> SassResult<Value> {
    inner_rgb("rgba", args, parser)
}

pub(crate) fn red(mut args: ArgumentResult, parser: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    match args.get_err(0, "color")? {
        Value::Color(c) => Ok(Value::Dimension {
            num: (c.red()),
            unit: Unit::None,
            as_slash: None,
        }),
        v => Err((
            format!("$color: {} is not a color.", v.inspect(args.span())?),
            args.span(),
        )
            .into()),
    }
}

pub(crate) fn green(mut args: ArgumentResult, parser: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    match args.get_err(0, "color")? {
        Value::Color(c) => Ok(Value::Dimension {
            num: (c.green()),
            unit: Unit::None,
            as_slash: None,
        }),
        v => Err((
            format!("$color: {} is not a color.", v.inspect(args.span())?),
            args.span(),
        )
            .into()),
    }
}

pub(crate) fn blue(mut args: ArgumentResult, parser: &mut Visitor) -> SassResult<Value> {
    args.max_args(1)?;
    match args.get_err(0, "color")? {
        Value::Color(c) => Ok(Value::Dimension {
            num: (c.blue()),
            unit: Unit::None,
            as_slash: None,
        }),
        v => Err((
            format!("$color: {} is not a color.", v.inspect(args.span())?),
            args.span(),
        )
            .into()),
    }
}

pub(crate) fn mix(mut args: ArgumentResult, parser: &mut Visitor) -> SassResult<Value> {
    args.max_args(3)?;
    let color1 = match args.get_err(0, "color1")? {
        Value::Color(c) => c,
        v => {
            return Err((
                format!("$color1: {} is not a color.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    let color2 = match args.get_err(1, "color2")? {
        Value::Color(c) => c,
        v => {
            return Err((
                format!("$color2: {} is not a color.", v.inspect(args.span())?),
                args.span(),
            )
                .into())
        }
    };

    let weight = match args.default_arg(
        2,
        "weight",
        Value::Dimension {
            num: (Number::from(50)),
            unit: Unit::None,
            as_slash: None,
        },
    ) {
        Value::Dimension { num: n, .. } if n.is_nan() => todo!(),
        Value::Dimension {
            num: n,
            unit: u,
            as_slash: _,
        } => bound!(args, "weight", n, u, 0, 100) / Number::from(100),
        v => {
            return Err((
                format!(
                    "$weight: {} is not a number.",
                    v.to_css_string(args.span(), parser.parser.options.is_compressed())?
                ),
                args.span(),
            )
                .into())
        }
    };
    Ok(Value::Color(Box::new(color1.mix(&color2, weight))))
}

pub(crate) fn declare(f: &mut GlobalFunctionMap) {
    f.insert("rgb", Builtin::new(rgb));
    f.insert("rgba", Builtin::new(rgba));
    f.insert("red", Builtin::new(red));
    f.insert("green", Builtin::new(green));
    f.insert("blue", Builtin::new(blue));
    f.insert("mix", Builtin::new(mix));
}
