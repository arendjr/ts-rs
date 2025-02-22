use proc_macro2::TokenStream;
use quote::quote;
use syn::{
    Field, FieldsNamed, GenericArgument, GenericParam, Generics, PathArguments, Result, Type,
};

use crate::attr::{FieldAttr, Inflection};
use crate::types::generics::format_type;
use crate::DerivedTS;

pub(crate) fn named(
    name: &str,
    rename_all: &Option<Inflection>,
    fields: &FieldsNamed,
    generics: &Generics,
) -> Result<DerivedTS> {
    let mut formatted_fields = vec![];
    let mut dependencies = vec![];
    for field in &fields.named {
        format_field(
            &mut formatted_fields,
            &mut dependencies,
            field,
            &rename_all,
            generics,
        )?;
    }

    let fields = quote!(vec![#(#formatted_fields),*].join("\n"));
    let generic_args = match &generics.params {
        params if !params.is_empty() => {
            let expanded_params = params
                .iter()
                .filter_map(|param| match param {
                    GenericParam::Type(type_param) => Some(type_param.ident.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(", ");
            quote!(format!("<{}>", #expanded_params))
        }
        _ => quote!("".to_owned()),
    };

    Ok(DerivedTS {
        inline: quote! {
            format!(
                "{{\n{}\n{}}}",
                #fields,
                " ".repeat(indent * 4)
            )
        },
        decl: quote!(format!("interface {}{} {}", #name, #generic_args, Self::inline(0))),
        inline_flattened: Some(fields),
        name: name.to_owned(),
        dependencies: quote! {
            let mut dependencies = vec![];
            #( #dependencies )*
            dependencies
        },
    })
}

// build an expresion which expands to a string, representing a single field of a struct.
fn format_field(
    formatted_fields: &mut Vec<TokenStream>,
    dependencies: &mut Vec<TokenStream>,
    field: &Field,
    rename_all: &Option<Inflection>,
    generics: &Generics,
) -> Result<()> {
    let FieldAttr {
        type_override,
        rename,
        inline,
        skip,
        optional,
        flatten,
    } = FieldAttr::from_attrs(&field.attrs)?;

    if skip {
        return Ok(());
    }

    let (ty, optional_annotation) = match optional {
        true => (extract_option_argument(&field.ty)?, "?"),
        false => (&field.ty, ""),
    };

    if flatten {
        match (&type_override, &rename, inline) {
            (Some(_), _, _) => syn_err!("`type` is not compatible with `flatten`"),
            (_, Some(_), _) => syn_err!("`rename` is not compatible with `flatten`"),
            (_, _, true) => syn_err!("`inline` is not compatible with `flatten`"),
            _ => {}
        }

        formatted_fields.push(quote!(<#ty as ts_rs::TS>::inline_flattened(indent)));
        dependencies.push(quote!(dependencies.append(&mut <#ty as ts_rs::TS>::dependencies());));
        return Ok(());
    }

    let formatted_ty = type_override.map(|t| quote!(#t)).unwrap_or_else(|| {
        if inline {
            dependencies
                .push(quote!(dependencies.append(&mut <#ty as ts_rs::TS>::dependencies());));
            quote!(<#ty as ts_rs::TS>::inline(indent + 1))
        } else {
            format_type(ty, dependencies, generics)
        }
    });
    let name = match (rename, rename_all) {
        (Some(rn), _) => rn,
        (None, Some(rn)) => rn.apply(&field.ident.as_ref().unwrap().to_string()),
        (None, None) => field.ident.as_ref().unwrap().to_string(),
    };

    formatted_fields.push(quote! {
        format!("{}{}{}: {},", " ".repeat((indent + 1) * 4), #name, #optional_annotation, #formatted_ty)
    });

    Ok(())
}

fn extract_option_argument(ty: &Type) -> Result<&Type> {
    match ty {
        Type::Path(type_path)
            if type_path.qself.is_none()
                && type_path.path.leading_colon.is_none()
                && type_path.path.segments.len() == 1
                && type_path.path.segments[0].ident == "Option" =>
        {
            let segment = &type_path.path.segments[0];
            match &segment.arguments {
                PathArguments::AngleBracketed(args) if args.args.len() == 1 => {
                    match &args.args[0] {
                        GenericArgument::Type(inner_ty) => Ok(&inner_ty),
                        _ => syn_err!("`Option` argument must be a type"),
                    }
                }
                _ => syn_err!("`Option` type must have a single generic argument"),
            }
        }
        _ => syn_err!("`optional` can only be used on an Option<T> type"),
    }
}
