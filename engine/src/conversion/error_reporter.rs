// Copyright 2020 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//    https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use syn::{ItemEnum, ItemStruct};

use super::{
    api::{AnalysisPhase, Api, ApiName, FuncToConvert, TypedefKind},
    convert_error::{ConvertErrorWithContext, ErrorContext},
    ConvertError,
};
use crate::types::{Namespace, QualifiedName};

/// Run some code which may generate a ConvertError.
/// If it does, try to note the problem in our output APIs
/// such that users will see documentation of the error.
pub(crate) fn report_any_error<F, T>(
    ns: &Namespace,
    apis: &mut Vec<Api<impl AnalysisPhase>>,
    fun: F,
) -> Option<T>
where
    F: FnOnce() -> Result<T, ConvertErrorWithContext>,
{
    match fun() {
        Ok(result) => Some(result),
        Err(ConvertErrorWithContext(err, None)) => {
            eprintln!("Ignored item: {}", err);
            None
        }
        Err(ConvertErrorWithContext(err, Some(ctx))) => {
            eprintln!("Ignored item {}: {}", ctx.to_string(), err);
            apis.push(ignored_item(ns, ctx, err));
            None
        }
    }
}

/// Run some code which generates an API. Add that API, or if
/// anything goes wrong, instead add a note of the problem in our
/// output API such that users will see documentation for the problem.
pub(crate) fn convert_apis<FF, SF, EF, TF, A, B: 'static>(
    in_apis: Vec<Api<A>>,
    out_apis: &mut Vec<Api<B>>,
    mut func_conversion: FF,
    mut struct_conversion: SF,
    mut enum_conversion: EF,
    mut typedef_conversion: TF,
) where
    A: AnalysisPhase,
    B: AnalysisPhase,
    FF: FnMut(
        ApiName,
        Box<FuncToConvert>,
        A::FunAnalysis,
        Option<QualifiedName>,
    ) -> Result<Box<dyn Iterator<Item = Api<B>>>, ConvertErrorWithContext>,
    SF: FnMut(
        ApiName,
        ItemStruct,
        A::StructAnalysis,
    ) -> Result<Box<dyn Iterator<Item = Api<B>>>, ConvertErrorWithContext>,
    EF: FnMut(
        ApiName,
        ItemEnum,
    ) -> Result<Box<dyn Iterator<Item = Api<B>>>, ConvertErrorWithContext>,
    TF: FnMut(
        ApiName,
        TypedefKind,
        Option<QualifiedName>,
        A::TypedefAnalysis,
    ) -> Result<Box<dyn Iterator<Item = Api<B>>>, ConvertErrorWithContext>,
{
    out_apis.extend(
        &mut in_apis
            .into_iter()
            .map(|api| {
                let tn = api.name().clone();
                let result: Result<Box<dyn Iterator<Item = Api<B>>>, ConvertErrorWithContext> =
                    match api {
                        // No changes to any of these...
                        Api::ConcreteType {
                            name,
                            rs_definition,
                            cpp_definition,
                        } => Ok(Box::new(std::iter::once(Api::ConcreteType {
                            name,
                            rs_definition,
                            cpp_definition,
                        }))),
                        Api::ForwardDeclaration { name } => {
                            Ok(Box::new(std::iter::once(Api::ForwardDeclaration { name })))
                        }
                        Api::StringConstructor { name } => {
                            Ok(Box::new(std::iter::once(Api::StringConstructor { name })))
                        }
                        Api::Const { name, const_item } => {
                            Ok(Box::new(std::iter::once(Api::Const { name, const_item })))
                        }
                        Api::CType { name, typename } => {
                            Ok(Box::new(std::iter::once(Api::CType { name, typename })))
                        }
                        Api::RustType { name, path } => {
                            Ok(Box::new(std::iter::once(Api::RustType { name, path })))
                        }
                        Api::RustFn { name, sig, path } => {
                            Ok(Box::new(std::iter::once(Api::RustFn { name, sig, path })))
                        }
                        Api::RustSubclassFn {
                            name,
                            subclass,
                            details,
                        } => Ok(Box::new(std::iter::once(Api::RustSubclassFn {
                            name,
                            subclass,
                            details,
                        }))),
                        Api::RustSubclassConstructor {
                            name,
                            subclass,
                            cpp_impl,
                            is_trivial,
                        } => Ok(Box::new(std::iter::once(Api::RustSubclassConstructor {
                            name,
                            subclass,
                            cpp_impl,
                            is_trivial,
                        }))),
                        Api::Subclass { name, superclass } => {
                            Ok(Box::new(std::iter::once(Api::Subclass {
                                name,
                                superclass,
                            })))
                        }
                        Api::IgnoredItem { name, err, ctx } => {
                            Ok(Box::new(std::iter::once(Api::IgnoredItem {
                                name,
                                err,
                                ctx,
                            })))
                        }
                        // Apply a mapping to the following
                        Api::Enum { name, item } => enum_conversion(name, item),
                        Api::Typedef {
                            name,
                            item,
                            old_tyname,
                            analysis,
                        } => typedef_conversion(name, item, old_tyname, analysis),
                        Api::Function {
                            name,
                            fun,
                            analysis,
                            name_for_gc,
                        } => func_conversion(name, fun, analysis, name_for_gc),
                        Api::Struct {
                            name,
                            item,
                            analysis,
                        } => struct_conversion(name, item, analysis),
                    };
                api_or_error(tn, result)
            })
            .flatten(),
    )
}

fn api_or_error<T: AnalysisPhase + 'static>(
    name: QualifiedName,
    api_or_error: Result<Box<dyn Iterator<Item = Api<T>>>, ConvertErrorWithContext>,
) -> Box<dyn Iterator<Item = Api<T>>> {
    match api_or_error {
        Ok(opt) => opt,
        Err(ConvertErrorWithContext(err, None)) => {
            eprintln!("Ignored {}: {}", name.to_string(), err);
            Box::new(std::iter::empty())
        }
        Err(ConvertErrorWithContext(err, Some(ctx))) => {
            eprintln!("Ignored {}: {}", name.to_string(), err);
            Box::new(std::iter::once(ignored_item(
                name.get_namespace(),
                ctx,
                err,
            )))
        }
    }
}

/// Run some code which generates an API for an item (as opposed to
/// a method). Add that API, or if
/// anything goes wrong, instead add a note of the problem in our
/// output API such that users will see documentation for the problem.
pub(crate) fn convert_item_apis<F, A, B: 'static>(
    in_apis: Vec<Api<A>>,
    out_apis: &mut Vec<Api<B>>,
    mut fun: F,
) where
    F: FnMut(Api<A>) -> Result<Box<dyn Iterator<Item = Api<B>>>, ConvertError>,
    A: AnalysisPhase,
    B: AnalysisPhase,
{
    out_apis.extend(
        in_apis
            .into_iter()
            .map(|api| {
                let tn = api.name().clone();
                let result = fun(api).map_err(|e| {
                    ConvertErrorWithContext(e, Some(ErrorContext::Item(tn.get_final_ident())))
                });
                api_or_error(tn, result)
            })
            .flatten(),
    )
}

fn ignored_item<A: AnalysisPhase>(ns: &Namespace, ctx: ErrorContext, err: ConvertError) -> Api<A> {
    Api::IgnoredItem {
        name: ApiName::new(ns, ctx.get_id().clone()),
        err,
        ctx,
    }
}
