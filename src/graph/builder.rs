//! # Graph Builder
//!
//! Transforms AST nodes into RDF triples following CodeOntology mappings.
//!
//! ## Ontology Mappings
//!
//! | Rust Construct | RDF Class              | Properties                    |
//! |----------------|------------------------|-------------------------------|
//! | struct         | code:Struct            | hasField, visibility          |
//! | enum           | code:Enum              | hasVariant, visibility        |
//! | trait          | code:Trait             | hasMethod, supertraits        |
//! | impl           | code:Implementation    | implements, forType           |
//! | fn             | co:Function            | hasParameter, returnType      |
//! | mod            | co:Module              | contains, visibility          |
//! | use            | code:Import            | importPath, importAlias       |

use crate::ontology::*;
use crate::parser::*;
use crate::pipeline::PipelineError;

/// Builder for constructing RDF graphs from AST nodes
pub struct GraphBuilder {
    project_id: String,
    triples: TripleSet,
    blank_node_counter: usize,
}

impl GraphBuilder {
    pub fn new(project_id: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            triples: TripleSet::new(),
            blank_node_counter: 0,
        }
    }

    /// Generate a unique blank node ID
    fn blank_node(&mut self) -> String {
        self.blank_node_counter += 1;
        format!("b{}", self.blank_node_counter)
    }

    /// Build a graph from a collection of AST nodes
    pub fn build_from_ast(mut self, nodes: &[AstNode]) -> Result<TripleSet, PipelineError> {
        // Create the project entity
        let project = Project::new(&self.project_id, &self.project_id);
        self.triples.extend(project.to_triples());

        // Process each top-level AST node
        for node in nodes {
            self.process_node(node, &project.iri)?;
        }

        Ok(self.triples)
    }

    fn process_node(&mut self, node: &AstNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        match node {
            AstNode::Module(m) => self.process_module(m, parent_iri),
            AstNode::Use(u) => self.process_use(u, parent_iri),
            AstNode::Struct(s) => self.process_struct(s, parent_iri),
            AstNode::Enum(e) => self.process_enum(e, parent_iri),
            AstNode::Trait(t) => self.process_trait(t, parent_iri),
            AstNode::Impl(i) => self.process_impl(i, parent_iri),
            AstNode::Function(f) => self.process_function(f, parent_iri),
            AstNode::Const(c) => self.process_const(c, parent_iri),
            AstNode::Static(s) => self.process_static(s, parent_iri),
            AstNode::TypeAlias(t) => self.process_type_alias(t, parent_iri),
            AstNode::Macro(m) => self.process_macro(m, parent_iri),
        }
    }

    fn process_module(&mut self, node: &ModuleNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let module = Module::new(parent_iri, &node.name)
            .with_visibility(convert_visibility(&node.visibility));

        let module_iri = module.iri.clone();

        // Add type and basic triples
        self.triples.extend(module.to_triples());

        // Add containment relationship
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_submodule(),
            module_iri.clone(),
        ));

        // Add documentation if present
        if let Some(doc) = &node.doc_comment {
            self.triples.add(Triple::new(
                module_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        // Add location if present
        if let Some(loc) = &node.location {
            self.add_location_triples(&module_iri, loc);
        }

        // Process child items
        for item in &node.items {
            self.process_node(item, &module_iri)?;
        }

        Ok(())
    }

    fn process_use(&mut self, node: &UseNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let import = Import::new(parent_iri, &node.path);
        let import_iri = import.iri.clone();

        self.triples.extend(import.to_triples());

        // Add containment
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_import(),
            import_iri.clone(),
        ));

        // Add glob marker if present
        if node.is_glob {
            self.triples.add(Triple::new(
                import_iri.clone(),
                CODE.iri("isGlobImport"),
                Literal::boolean(true),
            ));
        }

        // Add alias if present
        if let Some(alias) = &node.alias {
            self.triples.add(Triple::new(
                import_iri.clone(),
                CODE.iri("importAlias"),
                Literal::string(alias),
            ));
        }

        if let Some(loc) = &node.location {
            self.add_location_triples(&import_iri, loc);
        }

        Ok(())
    }

    fn process_struct(&mut self, node: &StructNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let strct = Struct::new(parent_iri, &node.name)
            .with_visibility(convert_visibility(&node.visibility));

        let struct_iri = strct.iri.clone();

        self.triples.extend(strct.to_triples());

        // Add containment
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_struct(),
            struct_iri.clone(),
        ));

        // Add tuple struct marker
        if node.is_tuple {
            self.triples.add(Triple::new(
                struct_iri.clone(),
                CODE.iri("isTupleStruct"),
                Literal::boolean(true),
            ));
        }

        // Add unit struct marker
        if node.is_unit {
            self.triples.add(Triple::new(
                struct_iri.clone(),
                CODE.iri("isUnitStruct"),
                Literal::boolean(true),
            ));
        }

        // Process generics
        self.add_generics(&struct_iri, &node.generics);

        // Process fields
        for field in &node.fields {
            self.process_field(field, &struct_iri)?;
        }

        // Add documentation
        if let Some(doc) = &node.doc_comment {
            self.triples.add(Triple::new(
                struct_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        // Add attributes
        for attr in &node.attributes {
            self.add_attribute(&struct_iri, attr);
        }

        // Add location
        if let Some(loc) = &node.location {
            self.add_location_triples(&struct_iri, loc);
        }

        Ok(())
    }

    fn process_enum(&mut self, node: &EnumNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let enm = Enum::new(parent_iri, &node.name)
            .with_visibility(convert_visibility(&node.visibility));

        let enum_iri = enm.iri.clone();

        self.triples.extend(enm.to_triples());

        // Add containment
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_enum(),
            enum_iri.clone(),
        ));

        // Process generics
        self.add_generics(&enum_iri, &node.generics);

        // Process variants
        for (idx, variant) in node.variants.iter().enumerate() {
            self.process_enum_variant(variant, &enum_iri, idx)?;
        }

        // Add documentation
        if let Some(doc) = &node.doc_comment {
            self.triples.add(Triple::new(
                enum_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        // Add location
        if let Some(loc) = &node.location {
            self.add_location_triples(&enum_iri, loc);
        }

        Ok(())
    }

    fn process_enum_variant(
        &mut self,
        variant: &EnumVariant,
        enum_iri: &Iri,
        index: usize,
    ) -> Result<(), PipelineError> {
        let variant_iri = Iri::new(format!("{}/variant/{}", enum_iri.as_str(), variant.name));

        // Type triple
        self.triples.add(Triple::new(
            variant_iri.clone(),
            rdf::type_(),
            CODE.iri("EnumVariant"),
        ));

        // Label
        self.triples.add(Triple::new(
            variant_iri.clone(),
            rdfs::label(),
            Literal::string(&variant.name),
        ));

        // Position
        self.triples.add(Triple::new(
            variant_iri.clone(),
            CODE.iri("variantIndex"),
            Literal::integer(index as i64),
        ));

        // Containment
        self.triples.add(Triple::new(
            enum_iri.clone(),
            CODE.iri("hasVariant"),
            variant_iri.clone(),
        ));

        // Discriminant
        if let Some(disc) = &variant.discriminant {
            self.triples.add(Triple::new(
                variant_iri.clone(),
                CODE.iri("discriminant"),
                Literal::string(disc),
            ));
        }

        // Documentation
        if let Some(doc) = &variant.doc_comment {
            self.triples.add(Triple::new(
                variant_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        // Process variant fields
        for field in &variant.fields {
            self.process_field(field, &variant_iri)?;
        }

        Ok(())
    }

    fn process_trait(&mut self, node: &TraitNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let trt = Trait::new(parent_iri, &node.name)
            .with_visibility(convert_visibility(&node.visibility));

        let trait_iri = trt.iri.clone();

        self.triples.extend(trt.to_triples());

        // Add containment
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_trait(),
            trait_iri.clone(),
        ));

        // Add unsafe marker
        if node.is_unsafe {
            self.triples.add(Triple::new(
                trait_iri.clone(),
                CODE.iri("isUnsafe"),
                Literal::boolean(true),
            ));
        }

        // Add auto marker
        if node.is_auto {
            self.triples.add(Triple::new(
                trait_iri.clone(),
                CODE.iri("isAutoTrait"),
                Literal::boolean(true),
            ));
        }

        // Process supertraits
        for supertrait in &node.supertraits {
            self.triples.add(Triple::new(
                trait_iri.clone(),
                reference::extends(),
                Literal::string(supertrait), // Ideally would be an IRI
            ));
        }

        // Process generics
        self.add_generics(&trait_iri, &node.generics);

        // Process trait items
        for item in &node.items {
            match item {
                TraitItem::Method(f) => {
                    self.process_function(f, &trait_iri)?;
                }
                TraitItem::Type(t) => {
                    self.process_associated_type(t, &trait_iri)?;
                }
                TraitItem::Const(c) => {
                    self.process_const(c, &trait_iri)?;
                }
            }
        }

        // Add documentation
        if let Some(doc) = &node.doc_comment {
            self.triples.add(Triple::new(
                trait_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        // Add location
        if let Some(loc) = &node.location {
            self.add_location_triples(&trait_iri, loc);
        }

        Ok(())
    }

    fn process_impl(&mut self, node: &ImplNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        // Named by what it implements, not by a processing-order counter, so the
        // IRI is stable across builds.
        let impl_id = iri_segment(&format!(
            "{}-for-{}",
            node.trait_name.as_deref().unwrap_or("inherent"),
            node.self_type.as_deref().unwrap_or("unknown")
        ));
        let impl_iri = Iri::new(format!("{}/impl/{}", parent_iri.as_str(), impl_id));

        // Type triple
        self.triples.add(Triple::new(
            impl_iri.clone(),
            rdf::type_(),
            CODE.iri("Implementation"),
        ));

        // Self type
        if let Some(self_type) = &node.self_type {
            self.triples.add(Triple::new(
                impl_iri.clone(),
                CODE.iri("forType"),
                Literal::string(self_type),
            ));
        }

        // Trait impl
        if let Some(trait_name) = &node.trait_name {
            self.triples.add(Triple::new(
                impl_iri.clone(),
                reference::implements(),
                Literal::string(trait_name),
            ));
        }

        // Unsafe
        if node.is_unsafe {
            self.triples.add(Triple::new(
                impl_iri.clone(),
                CODE.iri("isUnsafe"),
                Literal::boolean(true),
            ));
        }

        // Negative impl (! impl)
        if node.is_negative {
            self.triples.add(Triple::new(
                impl_iri.clone(),
                CODE.iri("isNegativeImpl"),
                Literal::boolean(true),
            ));
        }

        // Process generics
        self.add_generics(&impl_iri, &node.generics);

        // Process impl items
        for item in &node.items {
            match item {
                ImplItem::Method(f) => {
                    self.process_function(f, &impl_iri)?;
                }
                ImplItem::Type(t) => {
                    self.process_associated_type(t, &impl_iri)?;
                }
                ImplItem::Const(c) => {
                    self.process_const(c, &impl_iri)?;
                }
            }
        }

        // Add location
        if let Some(loc) = &node.location {
            self.add_location_triples(&impl_iri, loc);
        }

        Ok(())
    }

    fn process_function(&mut self, node: &FunctionNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let func = Function::new(parent_iri, &node.name)
            .with_visibility(convert_visibility(&node.visibility));

        let func_iri = func.iri.clone();

        self.triples.extend(func.to_triples());

        // Add containment
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_function(),
            func_iri.clone(),
        ));

        // Add modifiers
        if node.is_async {
            self.triples.add(Triple::new(
                func_iri.clone(),
                CODE.iri("isAsync"),
                Literal::boolean(true),
            ));
        }

        if node.is_const {
            self.triples.add(Triple::new(
                func_iri.clone(),
                CODE.iri("isConst"),
                Literal::boolean(true),
            ));
        }

        if node.is_unsafe {
            self.triples.add(Triple::new(
                func_iri.clone(),
                CODE.iri("isUnsafe"),
                Literal::boolean(true),
            ));
        }

        if node.is_extern {
            self.triples.add(Triple::new(
                func_iri.clone(),
                CODE.iri("isExtern"),
                Literal::boolean(true),
            ));
            if let Some(abi) = &node.abi {
                self.triples.add(Triple::new(
                    func_iri.clone(),
                    CODE.iri("abi"),
                    Literal::string(abi),
                ));
            }
        }

        // Return type
        if let Some(ret_type) = &node.return_type {
            self.triples.add(Triple::new(
                func_iri.clone(),
                reference::return_type(),
                Literal::string(ret_type),
            ));
        }

        // Process generics
        self.add_generics(&func_iri, &node.generics);

        // Process parameters
        for (index, param) in node.parameters.iter().enumerate() {
            self.process_parameter(param, index, &func_iri)?;
        }

        // Process function calls (call graph)
        for call in &node.body_calls {
            self.process_function_call(call, &func_iri)?;
        }

        // Add documentation
        if let Some(doc) = &node.doc_comment {
            self.triples.add(Triple::new(
                func_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        // Add attributes
        for attr in &node.attributes {
            self.add_attribute(&func_iri, attr);
        }

        // Add location
        if let Some(loc) = &node.location {
            self.add_location_triples(&func_iri, loc);
        }

        Ok(())
    }

    fn process_parameter(&mut self, param: &ParameterNode, index: usize, func_iri: &Iri) -> Result<(), PipelineError> {
        // Positional: the same signature yields the same IRIs in every build.
        let param_iri = Iri::new(format!("{}/param/{}", func_iri.as_str(), index));

        // Type triple
        self.triples.add(Triple::new(
            param_iri.clone(),
            rdf::type_(),
            CO.iri("Parameter"),
        ));

        // Name
        self.triples.add(Triple::new(
            param_iri.clone(),
            rdfs::label(),
            Literal::string(&param.name),
        ));

        // Containment
        self.triples.add(Triple::new(
            func_iri.clone(),
            containment::has_parameter(),
            param_iri.clone(),
        ));

        // Type annotation
        if let Some(ty) = &param.type_annotation {
            self.triples.add(Triple::new(
                param_iri.clone(),
                CODE.iri("typeAnnotation"),
                Literal::string(ty),
            ));
        }

        // Self parameter
        if param.is_self {
            self.triples.add(Triple::new(
                param_iri.clone(),
                CODE.iri("isSelfParameter"),
                Literal::boolean(true),
            ));
        }

        // Mutable
        if param.is_mutable {
            self.triples.add(Triple::new(
                param_iri.clone(),
                CODE.iri("isMutable"),
                Literal::boolean(true),
            ));
        }

        // Reference
        if param.is_reference {
            self.triples.add(Triple::new(
                param_iri.clone(),
                CODE.iri("isReference"),
                Literal::boolean(true),
            ));
        }

        Ok(())
    }

    fn process_function_call(&mut self, call: &FunctionCall, caller_iri: &Iri) -> Result<(), PipelineError> {
        // Create a call relationship
        self.triples.add(Triple::new(
            caller_iri.clone(),
            reference::calls(),
            Literal::string(&call.callee), // Ideally would resolve to an IRI
        ));

        Ok(())
    }

    fn process_field(&mut self, field: &FieldNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let field_entity = Field::new(parent_iri, &field.name)
            .with_visibility(convert_visibility(&field.visibility));

        let field_iri = field_entity.iri.clone();

        self.triples.extend(field_entity.to_triples());

        // Containment
        self.triples.add(Triple::new(
            parent_iri.clone(),
            containment::has_field(),
            field_iri.clone(),
        ));

        // Type annotation
        if let Some(ty) = &field.type_annotation {
            self.triples.add(Triple::new(
                field_iri.clone(),
                CODE.iri("typeAnnotation"),
                Literal::string(ty),
            ));
        }

        // Documentation
        if let Some(doc) = &field.doc_comment {
            self.triples.add(Triple::new(
                field_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        Ok(())
    }

    fn process_const(&mut self, node: &ConstNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let const_iri = Iri::new(format!("{}/const/{}", parent_iri.as_str(), node.name));

        // Type triple: a constant, or a module-level variable
        self.triples.add(Triple::new(
            const_iri.clone(),
            rdf::type_(),
            CODE.iri(if node.is_variable { "Variable" } else { "Constant" }),
        ));

        // Label
        self.triples.add(Triple::new(
            const_iri.clone(),
            rdfs::label(),
            Literal::string(&node.name),
        ));

        // Visibility
        self.triples.add(Triple::new(
            const_iri.clone(),
            CODE.iri("visibility"),
            Literal::string(visibility_to_string(&node.visibility)),
        ));

        // Type annotation
        if let Some(ty) = &node.type_annotation {
            self.triples.add(Triple::new(
                const_iri.clone(),
                CODE.iri("typeAnnotation"),
                Literal::string(ty),
            ));
        }

        // Documentation
        if let Some(doc) = &node.doc_comment {
            self.triples.add(Triple::new(
                const_iri.clone(),
                metadata::documentation(),
                Literal::string(doc),
            ));
        }

        if let Some(loc) = &node.location {
            self.add_location_triples(&const_iri, loc);
        }

        Ok(())
    }

    fn process_static(&mut self, node: &StaticNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let static_iri = Iri::new(format!("{}/static/{}", parent_iri.as_str(), node.name));

        // Type triple
        self.triples.add(Triple::new(
            static_iri.clone(),
            rdf::type_(),
            CODE.iri("StaticVariable"),
        ));

        // Label
        self.triples.add(Triple::new(
            static_iri.clone(),
            rdfs::label(),
            Literal::string(&node.name),
        ));

        // Mutable
        if node.is_mutable {
            self.triples.add(Triple::new(
                static_iri.clone(),
                CODE.iri("isMutable"),
                Literal::boolean(true),
            ));
        }

        if let Some(loc) = &node.location {
            self.add_location_triples(&static_iri, loc);
        }

        Ok(())
    }

    fn process_type_alias(&mut self, node: &TypeAliasNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let alias_iri = Iri::new(format!("{}/type/{}", parent_iri.as_str(), node.name));

        // Type triple
        self.triples.add(Triple::new(
            alias_iri.clone(),
            rdf::type_(),
            CODE.iri("TypeAlias"),
        ));

        // Label
        self.triples.add(Triple::new(
            alias_iri.clone(),
            rdfs::label(),
            Literal::string(&node.name),
        ));

        // Aliased type
        self.triples.add(Triple::new(
            alias_iri.clone(),
            CODE.iri("aliasedType"),
            Literal::string(&node.aliased_type),
        ));

        if let Some(loc) = &node.location {
            self.add_location_triples(&alias_iri, loc);
        }

        Ok(())
    }

    fn process_macro(&mut self, node: &MacroNode, parent_iri: &Iri) -> Result<(), PipelineError> {
        let macro_iri = Iri::new(format!("{}/macro/{}", parent_iri.as_str(), node.name));

        // Type triple
        self.triples.add(Triple::new(
            macro_iri.clone(),
            rdf::type_(),
            CODE.iri("Macro"),
        ));

        // Label
        self.triples.add(Triple::new(
            macro_iri.clone(),
            rdfs::label(),
            Literal::string(&node.name),
        ));

        // Macro kind
        if node.is_declarative {
            self.triples.add(Triple::new(
                macro_iri.clone(),
                CODE.iri("isDeclarativeMacro"),
                Literal::boolean(true),
            ));
        }

        if node.is_proc_macro {
            self.triples.add(Triple::new(
                macro_iri.clone(),
                CODE.iri("isProcMacro"),
                Literal::boolean(true),
            ));
        }

        Ok(())
    }

    fn process_associated_type(&mut self, node: &AssociatedType, parent_iri: &Iri) -> Result<(), PipelineError> {
        let type_iri = Iri::new(format!("{}/type/{}", parent_iri.as_str(), node.name));

        // Type triple
        self.triples.add(Triple::new(
            type_iri.clone(),
            rdf::type_(),
            CODE.iri("AssociatedType"),
        ));

        // Label
        self.triples.add(Triple::new(
            type_iri.clone(),
            rdfs::label(),
            Literal::string(&node.name),
        ));

        // Bounds
        for bound in &node.bounds {
            self.triples.add(Triple::new(
                type_iri.clone(),
                CODE.iri("typeBound"),
                Literal::string(bound),
            ));
        }

        // Default
        if let Some(default) = &node.default {
            self.triples.add(Triple::new(
                type_iri.clone(),
                CODE.iri("defaultType"),
                Literal::string(default),
            ));
        }

        Ok(())
    }

    fn add_generics(&mut self, parent_iri: &Iri, generics: &[GenericParam]) {
        for (idx, generic) in generics.iter().enumerate() {
            let generic_iri = Iri::new(format!(
                "{}/generic/{}",
                parent_iri.as_str(),
                generic.name
            ));

            // Type triple
            let type_class = match generic.kind {
                GenericKind::Type => CODE.iri("TypeParameter"),
                GenericKind::Lifetime => CODE.iri("LifetimeParameter"),
                GenericKind::Const => CODE.iri("ConstParameter"),
            };

            self.triples.add(Triple::new(
                generic_iri.clone(),
                rdf::type_(),
                type_class,
            ));

            // Label
            self.triples.add(Triple::new(
                generic_iri.clone(),
                rdfs::label(),
                Literal::string(&generic.name),
            ));

            // Position
            self.triples.add(Triple::new(
                generic_iri.clone(),
                CODE.iri("position"),
                Literal::integer(idx as i64),
            ));

            // Containment
            self.triples.add(Triple::new(
                parent_iri.clone(),
                reference::has_type_parameter(),
                generic_iri.clone(),
            ));

            // Bounds
            for bound in &generic.bounds {
                self.triples.add(Triple::new(
                    generic_iri.clone(),
                    CODE.iri("typeBound"),
                    Literal::string(bound),
                ));
            }

            // Default
            if let Some(default) = &generic.default {
                self.triples.add(Triple::new(
                    generic_iri.clone(),
                    CODE.iri("defaultValue"),
                    Literal::string(default),
                ));
            }
        }
    }

    fn add_location_triples(&mut self, subject: &Iri, loc: &SourceLocation) {
        self.triples.add(Triple::new(
            subject.clone(),
            metadata::start_line(),
            Literal::integer(loc.start_line as i64),
        ));

        self.triples.add(Triple::new(
            subject.clone(),
            metadata::end_line(),
            Literal::integer(loc.end_line as i64),
        ));

        self.triples.add(Triple::new(
            subject.clone(),
            metadata::start_column(),
            Literal::integer(loc.start_column as i64),
        ));

        self.triples.add(Triple::new(
            subject.clone(),
            metadata::file_path(),
            Literal::string(loc.file.to_string_lossy()),
        ));
    }

    fn add_attribute(&mut self, subject: &Iri, attr: &str) {
        self.triples.add(Triple::new(
            subject.clone(),
            metadata::has_attribute(),
            Literal::string(attr),
        ));
    }
}

// Helper functions

fn convert_visibility(vis: &crate::parser::Visibility) -> crate::ontology::Visibility {
    match vis {
        crate::parser::Visibility::Private => crate::ontology::Visibility::Private,
        crate::parser::Visibility::Public => crate::ontology::Visibility::Public,
        crate::parser::Visibility::Crate => crate::ontology::Visibility::Crate,
        crate::parser::Visibility::Super => crate::ontology::Visibility::Super,
        crate::parser::Visibility::Restricted(_) => crate::ontology::Visibility::Restricted(),
    }
}

fn visibility_to_string(vis: &crate::parser::Visibility) -> &'static str {
    match vis {
        crate::parser::Visibility::Private => "private",
        crate::parser::Visibility::Public => "pub",
        crate::parser::Visibility::Crate => "pub(crate)",
        crate::parser::Visibility::Super => "pub(super)",
        crate::parser::Visibility::Restricted(_) => "pub(in path)",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every named node can be placed: file, lines (finding: constants, statics,
    /// type aliases and imports used to carry no location at all).
    #[test]
    fn every_named_node_carries_its_location() {
        let at = |line| Some(SourceLocation::new("pkg/mod.py", line, 0, line, 10));
        let nodes = vec![
            AstNode::Const(ConstNode { name: "LIMIT".into(), visibility: crate::parser::Visibility::Public, type_annotation: None,
                value: Some("10".into()), is_variable: false, doc_comment: None, location: at(3) }),
            AstNode::Const(ConstNode { name: "logger".into(), visibility: crate::parser::Visibility::Public, type_annotation: Some("Logger".into()),
                value: None, is_variable: true, doc_comment: None, location: at(4) }),
            AstNode::Static(StaticNode { name: "COUNTER".into(), visibility: crate::parser::Visibility::Public, type_annotation: None,
                is_mutable: true, doc_comment: None, location: at(5) }),
            AstNode::TypeAlias(TypeAliasNode { name: "Id".into(), visibility: crate::parser::Visibility::Public, generics: vec![],
                aliased_type: "u64".into(), doc_comment: None, location: at(6) }),
            AstNode::Use(UseNode { path: "os.path".into(), alias: None, is_glob: false, visibility: crate::parser::Visibility::Private, location: at(1) }),
        ];
        let triples = GraphBuilder::new("p").build_from_ast(&nodes).unwrap();
        let facts = |label: &str| -> Vec<String> {
            let subject = triples.iter().find(|t| t.predicate.as_str().ends_with("rdf-schema#label") && t.object.to_string().contains(&format!("\"{}\"", label)))
                .unwrap_or_else(|| panic!("no node labelled {}", label)).subject.clone();
            triples.iter().filter(|t| t.subject == subject).map(|t| format!("{} {}", t.predicate.as_str().rsplit(['#', '/']).next().unwrap(), t.object)).collect()
        };
        for (label, line) in [("LIMIT", 3), ("logger", 4), ("COUNTER", 5), ("Id", 6)] {
            let f = facts(label);
            assert!(f.iter().any(|x| x.starts_with("filePath") && x.contains("pkg/mod.py")), "{} has no filePath: {:?}", label, f);
            assert!(f.iter().any(|x| x.starts_with("startLine") && x.contains(&line.to_string())), "{} has no startLine: {:?}", label, f);
        }
        assert!(facts("LIMIT").iter().any(|x| x.starts_with("type") && x.ends_with("Constant>")));
        assert!(facts("logger").iter().any(|x| x.starts_with("type") && x.ends_with("Variable>")), "a non-constant module binding is a Variable");
        let import_has_path = triples.iter().filter(|t| t.subject.to_string().contains("import")).any(|t| t.predicate.as_str().ends_with("filePath"));
        assert!(import_has_path, "imports carry their file");
    }

    #[test]
    fn build_simple_graph() {
        let nodes = vec![
            AstNode::Struct(StructNode {
                name: "Point".to_string(),
                visibility: crate::parser::Visibility::Public,
                generics: vec![],
                fields: vec![],
                is_tuple: false,
                is_unit: false,
                doc_comment: Some("A 2D point".to_string()),
                attributes: vec!["#[derive(Debug)]".to_string()],
                location: None,
            }),
        ];

        let builder = GraphBuilder::new("test-project");
        let triples = builder.build_from_ast(&nodes).unwrap();

        assert!(!triples.is_empty());

        // Check that we have a struct type triple
        let has_struct = triples.iter().any(|t| {
            t.object.to_string().contains("Struct")
        });
        assert!(has_struct);
    }

    #[test]
    fn build_function_graph() {
        let nodes = vec![
            AstNode::Function(FunctionNode {
                name: "calculate".to_string(),
                visibility: crate::parser::Visibility::Public,
                generics: vec![GenericParam {
                    name: "T".to_string(),
                    kind: GenericKind::Type,
                    bounds: vec!["Clone".to_string()],
                    default: None,
                }],
                parameters: vec![
                    ParameterNode {
                        name: "x".to_string(),
                        type_annotation: Some("T".to_string()),
                        is_self: false,
                        is_mutable: false,
                        is_reference: false,
                    },
                ],
                return_type: Some("T".to_string()),
                is_async: true,
                is_const: false,
                is_unsafe: false,
                is_extern: false,
                abi: None,
                body_calls: vec![],
                doc_comment: None,
                attributes: vec![],
                location: None,
            }),
        ];

        let builder = GraphBuilder::new("test-project");
        let triples = builder.build_from_ast(&nodes).unwrap();

        // Check for async marker
        let has_async = triples.iter().any(|t| {
            t.predicate.as_str().contains("isAsync")
        });
        assert!(has_async);

        // Check for type parameter
        let has_type_param = triples.iter().any(|t| {
            t.object.to_string().contains("TypeParameter")
        });
        assert!(has_type_param);
    }
}
