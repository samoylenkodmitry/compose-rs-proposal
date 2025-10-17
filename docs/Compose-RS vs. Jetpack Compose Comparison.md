

# **An Architectural Blueprint for a Rust-Native, Compose-Inspired UI Framework**

## **Part I: Deconstruction of the Jetpack Compose Paradigm**

The foundation of any successful technological translation lies in a first-principles understanding of the source paradigm. Before proposing an architecture for a Rust-native declarative UI framework, it is imperative to deconstruct its inspiration, Google's Jetpack Compose. This analysis moves beyond a surface-level API review to dissect the core architectural decisions, runtime mechanics, and compiler integrations that collectively enable its expressive power and performance. By understanding the *why* behind Compose's design, we can identify the principles that are fundamental and must be preserved, versus those that are implementation artifacts of its Kotlin/JVM environment and can be reimagined to leverage the unique strengths of Rust.

### **1.1 The Declarative Model and Layered Architecture: A Philosophy of Composition**

The most fundamental principle of Jetpack Compose is its embrace of a purely declarative UI model, encapsulated in the equation .1 This paradigm posits that the user interface is not a persistent collection of mutable widget objects to be imperatively manipulated, but rather a stateless function that transforms application state into a description of the UI.2 Each time the state changes, this function is re-invoked, producing a new UI description. This model fundamentally simplifies UI development by eliminating entire classes of bugs common in imperative systems, such as those arising from inconsistent state or manual view manipulation.4

This philosophy is embodied in a carefully designed, layered architecture. Jetpack Compose is not a monolithic entity but a stack of modules, where each layer builds upon the public APIs of the layers below it. This structure provides a gradient of abstraction, allowing developers to operate at the level most appropriate for their task while minimizing dependencies.6

The four major architectural layers are:

1. **Runtime:** This is the foundational, platform-agnostic layer. It provides the core mechanics of the Compose programming model, including the @Composable annotation that marks functions for transformation by the compiler, the remember API for maintaining state across invocations, and the mutableStateOf primitive for creating observable state holders.6 The runtime's capabilities are general enough that they could be used for managing any tree-like data structure, not just UI elements.7  
2. **UI:** This layer builds on the runtime to introduce the fundamental concepts of a UI toolkit. It is composed of multiple sub-modules (e.g., ui-text, ui-graphics) and defines primitives such as LayoutNode (the core element of the UI tree), Modifier (for decorating components), input handlers, and custom drawing and layout logic.6  
3. **Foundation:** This layer provides a set of design-system-agnostic building blocks. It includes essential layout components like Row and Column, performant list components like LazyColumn, and gesture recognition systems.6 This layer serves as the ideal base for developers wishing to construct their own bespoke design systems.  
4. **Material:** The highest level of the core stack, this layer provides a comprehensive and opinionated implementation of Google's Material Design system. It includes a theming system, a rich library of styled components (buttons, cards, etc.), ripple effects, and icons.6

A key strength of this architecture is its principle of **assembly over inheritance**. High-level components are not opaque, monolithic classes but are themselves composable functions assembled from lower-level primitives. For instance, a Material Button is constructed from a Surface (for background and shape), a CompositionLocalProvider (for managing content alpha on disable), ProvideTextStyle (for theming), and a Row (for content layout).6

This architectural choice represents a significant inversion of control compared to traditional UI toolkits. In an imperative toolkit, customizing a button beyond its exposed API often requires subclassing, which can be complex and brittle. In Compose, a developer can create a completely custom button by simply replicating the assembly of primitives, substituting or modifying elements as needed. This makes the framework inherently more flexible and extensible. However, this power comes with the responsibility of choosing the correct level of abstraction. The official guidance cautions developers to not always reach for the lowest-level building blocks, as doing so may mean forgoing the benefits of higher-level components and losing future updates from the upstream implementation.6 For any proposed compose-rs framework, establishing these clear architectural layers and providing robust guidance on their use will be a critical factor for adoption and long-term maintainability.

### **1.2 The Recomposition Engine: State, Identity, and Stability**

The mechanism that brings the declarative UI \= f(state) model to life is the **recomposition engine**. The process begins with an *initial composition*, where Compose executes the root composable functions for the first time, building a tree-like data structure called a Composition that describes the UI.9 When the application's state changes, Compose does not redraw the entire screen. Instead, it schedules a *recomposition*.2

Recomposition is the intelligent re-execution of only those composable functions that may have been affected by the state change.9 This process is triggered when a composable reads a value from an observable state holder, most commonly an object of type State\<T\>.2 Compose seamlessly integrates with other reactive streams from the Android ecosystem, such as Kotlin's Flow or LiveData, by providing utility functions like collectAsStateWithLifecycle() that convert them into a State\<T\> that the recomposition engine can track.2

The performance of this system hinges on its ability to skip as much work as possible. During recomposition, Compose can skip the execution of a composable function entirely if its inputs have not changed since the previous composition.9 This is an optimistic process; if a new state change occurs while a recomposition is in progress, the current one may be canceled in favor of a new one based on the more recent state.11

To determine whether to skip a composable, the runtime must be able to uniquely identify it across compositions and compare its inputs. Identity is determined not by the function's name, but by its **call site**—its location in the source code—and its **order of execution** within its parent.9 This technique, known as positional memoization, is highly efficient but has a critical weakness: in dynamic collections (like a list of items), reordering, adding, or removing items can change the execution order of subsequent items. This can cause Compose to mistakenly associate the state of one item with the composable of another, leading to incorrect UI and poor performance as it unnecessarily recomposes elements whose data has not actually changed.9 To solve this, Compose provides the key composable, which allows developers to provide an explicit, stable identifier (e.g., a unique ID from a database) for a composable, ensuring its identity is preserved regardless of its position.9

For input comparison to be meaningful, Compose relies on a formal contract known as **stability**. A type is considered stable if it adheres to three rules:

1. The result of equals() for two instances will forever be the same for the same two instances.  
2. If a public property of the type changes, the Composition will be notified.  
3. All public property types are also stable.9

The Compose compiler can infer stability for many common types, including all primitives, strings, and function types (lambdas).12 Immutable data classes whose properties are all stable types are the ideal structure for state objects, as they inherently satisfy the contract.2 If the compiler cannot prove a type is stable, it will be treated as unstable, and composables that accept it as a parameter will be re-executed on every recomposition of their parent, even if the instance has not changed. This can be a significant and non-obvious source of performance issues.

The entire concept of stability is a sophisticated workaround for the realities of a garbage-collected (GC) environment like the JVM. In Kotlin, two distinct object instances can be considered equal (via the equals method) but are not referentially identical. If the recomposition engine only checked for referential identity, it would almost never be able to skip recomposition for composables that take objects as parameters, as state updates typically produce new object instances. Therefore, it must rely on equals(). However, a simple equals() check is insufficient if an object's properties are mutable, as the object could change *after* the check without notifying Compose. The stability contract is thus a promise to the runtime about a type's behavior. This is a necessary complexity in the Kotlin/JVM world, but it introduces a "leaky abstraction" where developers must be aware of this internal mechanism to write performant code.

This presents a significant opportunity for a Rust-based implementation. Rust's ownership model provides compile-time guarantees about mutability. The distinction between an immutable reference (\&T) and a mutable reference (\&mut T) is enforced by the compiler. A compose-rs framework could leverage these guarantees to create a more direct and less "magical" skipping mechanism. If a composable accepts only immutable data that implements the standard PartialEq trait, the framework can be certain that the data cannot have changed in an unobserved way. This could potentially eliminate the need for a complex, heuristic-based stability inference system, making the performance model simpler and more predictable for developers.

### **1.3 The Compiler as a Core Framework Component**

Jetpack Compose is not merely a runtime library; it is a hybrid system in which the Kotlin compiler is an integral and active participant.7 The declarative syntax and stateful nature of composable functions are enabled by a powerful Kotlin compiler plugin that transforms the code at compile time.

This plugin intercepts both the compiler's frontend (supporting the original K1 and the new K2/FIR architectures) and its backend (which operates on Intermediate Representation, or IR).16 At a high level, the plugin rewrites every function annotated with @Composable. It injects parameters, such as a Composer object, which are invisible to the developer but essential for the runtime. It wraps calls to other composables with tracking logic, enabling the runtime to build the composition tree. Crucially, it transforms state management calls like remember and inserts the memoization checks that compare inputs and enable the intelligent skipping of functions during recomposition.8

The developer-facing API is thus a carefully crafted illusion. A developer writes var count by remember { mutableStateOf(0) }, which appears to be a simple local variable declaration.17 However, this variable must persist its value across multiple invocations of the function. A standard Kotlin function has no such memory. The compiler plugin transforms this declaration into something far more complex. The generated code does not create a simple local variable on the stack. Instead, it interacts with the implicit Composer object and its associated data structure, often called a "slot table." The generated logic effectively performs the following steps: "At this function's current position in the composition tree, query the slot table for a stored value. If a value exists (indicating a recomposition), return it. If no value exists (indicating the initial composition), execute the lambda (mutableStateOf(0)), store the result in the slot table at the current position, and then return it." This transformation is the "magic" that enables the simple, declarative syntax.

Recognizing the deep symbiosis between the language and the framework, a major strategic shift occurred with the release of Kotlin 2.0: the Jetpack Compose compiler was merged into the main Kotlin language repository.18 This move ensures that for every release of the Kotlin compiler, a perfectly compatible Compose compiler is released simultaneously.20 This eliminates a significant developer pain point of needing to find and manage compatible versions of the Kotlin language plugin and the Compose compiler extension.19 To further streamline this integration, a new Gradle plugin, org.jetbrains.kotlin.plugin.compose, was introduced. This plugin automates the configuration, replacing the manual kotlinCompilerExtensionVersion property with a type-safe composeCompiler {} configuration block in the build script.19

This deep integration poses a challenge and an opportunity for a Rust implementation. Replicating a full compiler plugin that integrates with rustc is a monumental task, reserved for only the most ambitious projects. However, Rust offers an alternative and more idiomatic mechanism for code transformation: **procedural macros**. A compose-rs framework could leverage attribute macros (\#\[composable\]) to perform similar transformations on Rust functions at compile time. While potentially less powerful than a full compiler plugin, a macro-based approach would be more transparent, easier for the community to understand and contribute to, and would avoid the maintenance overhead of tracking changes in the internal rustc APIs. The central architectural question for compose-rs is whether Rust's macro system is powerful enough to provide the same level of ergonomic simplicity that Compose's compiler plugin provides for Kotlin.

### **1.4 State Management and Unidirectional Data Flow (UDF)**

Jetpack Compose is architecturally aligned with the principles of **Unidirectional Data Flow (UDF)**. This design pattern dictates that in a UI system, state should flow down from higher-level components to lower-level ones, and events should flow up from lower-level components to the ones that own the state.1 This creates a clear, predictable, and traceable data flow that decouples the components responsible for displaying state (the UI) from the components responsible for managing it (state holders).24

The primary mechanism for implementing UDF in Compose is a pattern known as **state hoisting**. This involves making UI components stateless. A stateful composable, which manages its own state internally using remember, is often less reusable and harder to test.2 To make it stateless, its internal state is "hoisted" up to its parent. The now-stateless child component receives the current state via its parameters and exposes callbacks (typically lambdas) to notify the parent of events that should trigger a state change.2

The canonical pattern is to replace an internal state variable var value: T with two parameters in the function signature:

* value: T: The current value to display.  
* onValueChange: (T) \-\> Unit: An event callback that requests the value be changed to a new one.2

This approach yields several critical benefits:

* **Single Source of Truth:** By moving the state to a common owner instead of duplicating it, there is only one authoritative source for that piece of data, which helps prevent bugs from inconsistent states.2  
* **Encapsulation:** The logic for modifying the state is encapsulated within the state-owning parent. The stateless child can only request changes, not perform them directly.2  
* **Shareability and Interceptability:** The hoisted state can be shared with multiple children, and the parent can choose to intercept, modify, or ignore events from children before updating the state.2  
* **Decoupling:** The stateless component becomes completely decoupled from where its state is stored. The state could be in the immediate parent, a higher-level composable, or, as is common in larger applications, in an architecture component like an Android ViewModel.2

At the screen level, state is typically hoisted into ViewModel classes. These are special classes provided by the Android framework that are designed to store and manage UI-related data in a lifecycle-conscious way, surviving configuration changes like screen rotations.24 The ViewModel then exposes the UI state to the composables through observable holders like StateFlow.4

The UDF pattern is not merely a stylistic recommendation; it is an architectural necessity for achieving optimal performance in Compose. The recomposition engine's skipping mechanism relies on checking if a composable's input parameters have changed.9 In a proper UDF architecture, a parent holds the state. An event from a child triggers a state update in the parent. This creates a new state object. During the subsequent recomposition, the parent re-runs and passes this new state object down to its children. The recomposer sees that the child's input parameter has changed and correctly re-runs it, while other children that received unchanged state are skipped. The data flow is explicit, and the dependency graph is clear to the runtime.

Violating this pattern—for example, by passing a complex, mutable object down the tree and allowing a child to modify its properties directly—can break the performance model.23 From the parent's perspective, the reference to the object it passed down has not changed, so it may incorrectly assume its children do not need to be recomposed. This can lead to UI inconsistencies where changes are not reflected correctly across all dependent components. Therefore, the "state flows down, events flow up" mantra is deeply intertwined with the core mechanics of the recomposition engine. A compose-rs framework must strongly encourage a similar pattern. Fortunately, Rust's borrow checker may naturally guide developers toward this architecture. Passing a mutable reference (\&mut T) down a deep and complex UI tree is often ergonomically difficult and un-idiomatic in Rust, making a message-passing approach where children communicate intent via event callbacks (Fn()) a more natural and safer alternative.

## **Part II: The Rust Advantage: Language and Ecosystem**

Having deconstructed the core principles of Jetpack Compose, the analysis now pivots to the target environment: Rust. A simple one-to-one translation of Compose's architecture would fail to capitalize on Rust's unique features and would likely struggle against its constraints. A successful compose-rs must be a reimagination, built idiomatically to leverage the language's strengths in performance, memory safety, and concurrency. This section evaluates the foundational differences between Rust and Kotlin and surveys the existing Rust UI ecosystem to identify established patterns, opportunities for innovation, and potential pitfalls to avoid.

### **2.1 Foundational Language Comparison: Rust vs. Kotlin for UI Frameworks**

The choice of programming language has profound implications for the architecture of a UI framework. The differences between Rust and Kotlin, particularly in memory management and concurrency, dictate fundamentally different approaches to framework design.

The most significant differentiator is **memory management**. Kotlin, when targeting Android and the JVM, relies on a highly optimized, concurrent garbage collector (GC).28 While modern GCs are remarkably efficient, they introduce an element of non-determinism. GC pauses, however brief, can occur at any time, and if a pause exceeds the frame budget of the display (e.g., 16.6ms for a 60Hz display), it will result in a dropped frame, perceived by the user as "jank" or a stutter.29 Rust, in contrast, manages memory at compile time through its system of ownership, borrowing, and lifetimes.30 This approach guarantees memory safety without the need for a runtime GC, eliminating this entire category of non-deterministic pauses and enabling highly predictable performance, a critical attribute for consistently smooth user interfaces.28

This difference in memory models directly impacts **performance and concurrency**. Rust is renowned for performance that rivals C and C++, making it an excellent candidate for performance-critical tasks like rendering.29 Its "fearless concurrency" model is a key advantage; the compiler statically prevents data races, allowing developers to write multi-threaded code with a high degree of confidence.31 This is particularly valuable for a UI framework, which must often perform work (like data fetching or processing) on background threads without corrupting the UI state. Kotlin's performance on the JVM is excellent, but it generally operates at a higher level of abstraction with some runtime overhead.28 Its concurrency model is built around coroutines, a powerful and ergonomic abstraction for asynchronous programming that is well-suited for UI applications.31

The trade-off for Rust's performance and safety guarantees is its **developer experience and learning curve**. The ownership model is a novel concept for many programmers and presents a steep initial learning curve.28 The Rust compiler is famously strict, though its error messages are exceptionally helpful. Conversely, Kotlin was designed with developer productivity as a primary goal. It has a gentle learning curve, especially for developers with a Java background, and its concise, expressive syntax and features like built-in null safety are widely praised.31

The core trade-off in choosing between these languages for a UI framework is one of **predictability versus productivity**. Kotlin and Jetpack Compose prioritize developer productivity, providing a highly ergonomic API that hides much of the underlying complexity, at the cost of relying on a GC and a complex stability system to manage performance. Rust prioritizes performance and control, offering the potential for a UI with a perfectly predictable performance profile, but at the cost of requiring the developer—and the framework architect—to contend with the complexities of the borrow checker.

A successful compose-rs framework cannot fight against the borrow checker; it must be designed to embrace it. The architecture must be idiomatic to Rust. This means that patterns that are common in GC languages, such as passing mutable object graphs down a component tree, are likely to be unworkable. Instead, the framework should favor architectures that align with Rust's ownership rules. This suggests that a message-passing system for state updates, similar to the Elm architecture, would be a natural fit. The framework's core value proposition would not be just "Compose in Rust," but "A UI framework that offers the declarative ergonomics of Compose with the performance guarantees and memory safety of Rust."

### **2.2 Survey of the Rust Declarative UI Landscape**

The Rust UI ecosystem is a dynamic and rapidly evolving space. While no single framework has achieved the dominance of React in the web world or SwiftUI on Apple platforms, a number of serious contenders have emerged, each exploring different architectural trade-offs. An analysis of this landscape is essential for positioning compose-rs, allowing it to learn from established patterns and innovate where opportunities exist.34 The ecosystem is fragmented, but there is a clear convergence on key principles: a preference for declarative APIs, a move towards custom, cross-platform rendering using libraries like wgpu, and an unwavering focus on performance.34

| Framework | Architectural Pattern | State Management | Rendering Model | Primary Use Case | Key Differentiator |
| :---- | :---- | :---- | :---- | :---- | :---- |
| **Iced** | Elm Architecture (Retained Mode) | Centralized via Messages (UDF) | Custom (wgpu, tiny-skia), Renderer-Agnostic Core | General Purpose Desktop Apps | Strict adherence to the Elm pattern; type-safe messages.36 |
| **Slint** | Custom DSL \+ AOT Compilation | Reactive Property Bindings | Custom (OpenGL, Skia, Software), Pluggable Backends | Embedded Systems, Desktop | .slint markup language separates UI from logic; strong tooling.38 |
| **egui** | Immediate Mode | Implicit/Local, Rebuilt Each Frame | Immediate Mode Redraw, Integrates with other engines | Debug Tools, Game UIs, Prototyping | Simplicity of the immediate-mode programming model.40 |
| **Druid (Legacy)** | Data-Oriented (Retained Mode) | Data-binding via Data and Lens traits | Custom (Piet 2D library) | Experimental Desktop Apps | Data-oriented, non-Elm approach; now discontinued.42 |
| **WaterUI** | SwiftUI-inspired, Declarative | Fine-grained Reactivity | Native Widget Wrapping (SwiftUI, GTK4) | Cross-Platform Native Apps | Aims for true native look-and-feel by wrapping OS widgets.44 |

**Analysis of Key Frameworks:**

* **Iced:** As a mature framework inspired by Elm, Iced provides a strong precedent for a Rust-native, retained-mode UI toolkit.36 Its architecture is built around a strict Model-View-Update loop, where a central state is modified exclusively in response to type-safe messages.37 This pattern is exceptionally well-suited to Rust's ownership model, as it avoids the complexities of shared mutable state. Its modular design, featuring a renderer-agnostic core with default backends for wgpu and the CPU-based tiny-skia, is a model of good engineering.37  
* **Slint:** Slint's most distinctive feature is its use of a dedicated Domain-Specific Language (DSL) in .slint files, which is reminiscent of QML or HTML.38 This allows for a clean separation of the UI's appearance from the application's business logic. The Slint compiler processes these .slint files ahead-of-time, generating highly optimized native code for various target languages, including Rust.38 The framework is notable for its excellent tooling, including a live-preview feature and a Figma plugin, and its focus on efficiency, with a runtime that can operate in under 300KiB of RAM, making it ideal for embedded systems.38  
* **egui:** In contrast to the retained-mode approach of Iced and Slint, egui is an immediate-mode GUI library.40 This means the entire UI is conceptually rebuilt and redrawn from scratch every frame. This greatly simplifies state management, as there is no persistent tree of widget objects to manipulate. While this can be less power-efficient for static applications, it is an extremely productive and straightforward model for UIs that are constantly changing, such as in-game debug menus or data visualization tools.48  
* **Tauri and Web-based Frameworks:** A significant portion of the Rust UI space is occupied by frameworks like Tauri and Dioxus, which leverage web technologies for the front end.41 These frameworks typically use a system webview to render HTML, CSS, and JavaScript, while the application's backend logic runs in Rust. While this is a powerful and popular approach for building cross-platform desktop applications, it relies on a completely different rendering stack and programming model than the native-rendering approach of Jetpack Compose.  
* **Emerging Trends (WaterUI):** Newer frameworks like WaterUI are exploring a different path to achieving a "native" experience. Instead of implementing a custom rendering pipeline, WaterUI aims to be a thin, declarative Rust API that wraps the platform's native UI toolkits, such as SwiftUI on Apple platforms and GTK on Linux.44 This approach prioritizes native look, feel, and accessibility over cross-platform pixel-perfect consistency.

This survey reveals that compose-rs would enter a vibrant but unconsolidated market. It does not need to invent its core concepts in a vacuum. It can synthesize the best ideas from the ecosystem: the borrow-checker-friendly message-passing architecture of Iced, the ergonomic and toolable developer experience of Slint (but implemented via procedural macros instead of a separate DSL), and the modern, GPU-accelerated rendering approach centered on wgpu. By doing so, compose-rs can position itself not as just another Rust UI framework, but as a comprehensive solution that uniquely combines the proven component and recomposition model of Jetpack Compose with the performance and safety of idiomatic Rust.

## **Part III: Architectural Blueprint for compose-rs**

Synthesizing the deconstruction of Jetpack Compose and the analysis of the Rust ecosystem, this section presents a concrete architectural blueprint for compose-rs. This proposal is not a direct port; it is a reimagination of Compose's principles, adapted to be idiomatic and to leverage the unique strengths of the Rust programming language. The goal is a framework that offers a familiar, component-based developer experience while delivering the performance, memory safety, and predictability that are Rust's hallmarks.

| Feature | Jetpack Compose (Kotlin/JVM) | Proposed compose-rs (Rust) | Rationale for Change |
| :---- | :---- | :---- | :---- |
| **Core Language** | Kotlin | Rust | Leverage performance, memory safety, and fearless concurrency. |
| **Memory Model** | Garbage Collection (GC) | Ownership & Borrowing | Eliminate GC pauses for predictable, real-time performance. |
| **Recomposition Trigger** | Reading State\<T\> | Reading State\<T\> (via interior mutability) | Maintain the core reactive principle of state-driven updates. |
| **State Primitives** | remember { mutableStateOf(value) } | \`let value \= state(cx, |  |
| initial\_value)\` | Adapt syntax to be idiomatic Rust; cx provides context. |  |  |
| **Compiler Integration** | Deep Kotlin Compiler Plugin | Procedural Attribute Macro (\#\[composable\]) | Avoid deep rustc integration; use Rust's standard, powerful metaprogramming features. |
| **Performance Optimization** | Stability System & Memoization | PartialEq checks & Fine-Grained Reactivity | Leverage ownership to simplify skipping logic; improve efficiency by updating nodes instead of whole functions. |

### **3.1 Core Runtime and Composition Model in Rust**

The heart of the framework will be its core runtime, which manages the composition tree and the state of the UI. This system must provide the "magic" that makes declarative, stateful components possible, but using idiomatic Rust mechanisms.

The \#\[composable\] Macro and the Context:  
Instead of a @Composable annotation processed by a closed-source compiler plugin, the proposal is to use a \#\[composable\] procedural attribute macro. This is a standard, powerful feature of the Rust ecosystem. This macro would transform an ordinary Rust function, for example fn my\_widget(cx: \&mut Context, name: \&str), into a component that can participate in the composition. The macro's transformation would be responsible for:

1. Injecting code at the beginning and end of the function to interact with the runtime's composition engine.  
2. Rewriting calls to state management primitives like remember and state to interact with the Context.  
3. Managing the positional memoization required to track component identity.

The Context (or Composer) object would be an explicit parameter passed to every composable function. It serves as the handle to the runtime, providing access to the underlying slot table and the recomposition scheduler.

The Composition Tree:  
The UI's structure will be represented internally as a tree of nodes, analogous to Compose's LayoutNode tree.50 To manage memory efficiently and avoid complex lifetime annotations that would plague a tree of heap-allocated, interconnected nodes, this tree should be implemented using an arena allocator. A crate like indextree is a suitable candidate, storing all nodes in a contiguous Vec and representing parent-child relationships with indices. This approach improves cache locality and simplifies memory management.  
Implementing remember and State Primitives:  
The remember functionality, crucial for preserving state across recompositions, will be provided as a function that takes the Context and a closure. \`remember(cx, |  
| { /\* expensive calculation \*/ }). The macro-transformed code would use the Context\` to access a slot table. During initial composition, it would execute the closure, store the result in the table, and return it. During recomposition, it would simply retrieve and return the already-stored value.2

For reactive state, we propose a state() function: \`let (count, set\_count) \= state(cx, |

| 0);. This would be the compose-rsequivalent ofremember { mutableStateOf(0) }.2 It would use rememberinternally to store a state-holding object. This object, let's call itState, would use interior mutability (e.g., Rc\<RefCell\>for single-threaded contexts, orArc\<RwLock\>\` for multi-threaded ones) to hold the value. The getter would register the current component as a subscriber to this state, and the setter would notify the runtime's scheduler that all subscribers need to be recomposed. This encapsulates the unsafe aspects of mutability within the primitive, exposing a safe and ergonomic API to the user.

### **3.2 A High-Performance, GC-Free Recomposition System**

A key advantage of building in Rust is the opportunity to create a recomposition system that is more performant and predictable than its GC-based counterpart.

Fine-Grained Reactivity:  
Jetpack Compose's recomposition model operates at the function level. When a state value changes, the entire body of any subscribing composable function is re-executed. We propose an improvement inspired by more recent reactive frameworks: fine-grained reactivity. Instead of re-running entire functions, the system could, where possible, update only the specific parts of the composition tree that have changed. For example, if a Text component displays a count state, a change in count should ideally only trigger the logic needed to update the text content of that specific node, not re-execute the entire parent function that contains the Text component. This can be achieved by having the state() primitive's getter return a reactive signal that the rendering system can subscribe to directly.  
Leveraging Ownership for Simplified Skipping:  
The complex stability system of Jetpack Compose is a necessary adaptation to the JVM's memory model. In Rust, this can be drastically simplified. The recomposition engine can rely on Rust's standard PartialEq trait for change detection. When a \#\[composable\] function is called, the runtime can compare its current inputs to its previous inputs using \==. Because Rust's ownership and borrowing rules prevent unobserved mutation of immutable data, this check is a far more reliable signal that the component's inputs have not semantically changed. This would eliminate an entire class of subtle performance bugs related to "unstable" types and remove the need for developers to reason about a framework-specific concept like stability, making the performance model more transparent.  
The Scheduler:  
At the core of the runtime will be a scheduler. When a state object is mutated (e.g., via set\_count(1)), it will not trigger an immediate re-render. Instead, it will add the component's ID to a "dirty set" within the scheduler. The scheduler will then, typically synchronized with the display's refresh cycle (e.g., via requestAnimationFrame on the web or a platform-specific equivalent), process this dirty set. It will traverse the composition tree, execute the update logic for all marked components, and generate a set of changes to be applied to the UI. This batching mechanism is critical for performance, ensuring that multiple state changes within a single frame result in only one UI update.9

### **3.3 Rendering Strategy and Multi-Platform Architecture**

To be a viable modern framework, compose-rs must target all major platforms: desktop, web, and mobile. This requires a modular rendering architecture.

Renderer-Agnostic Core:  
Following the successful design of frameworks like Iced, the core compose-rs crate should be completely renderer-agnostic.37 It will define the composition model, the layout system (e.g., a Flexbox-like model), and a trait-based abstraction for a renderer. This renderer trait would define a set of drawing primitives (e.g., draw\_rect, draw\_text, apply\_clip). This separation allows different backends to be developed for different platforms or rendering technologies.  
Default wgpu Backend:  
The primary, officially supported renderer should be built on wgpu. wgpu is a Rust-native graphics abstraction that provides a modern, safe API over platform-specific graphics libraries like Vulkan, Metal, DirectX 12, and OpenGL.37 Building on wgpu immediately provides high-performance, GPU-accelerated rendering across all major desktop platforms (Windows, macOS, Linux).  
WebAssembly (WASM) Target:  
WASM must be a first-class citizen in the architecture.51 The core runtime library should be designed to be no\_std compatible where feasible. A dedicated WASM rendering backend would be created that uses wgpu's ability to target WebGL/WebGPU, rendering to an HTML \<canvas\> element. A significant challenge for WASM-based UIs is the initial binary size, as the entire framework and application code must be downloaded by the browser.53 The architecture should therefore prioritize modularity and tree-shaking to keep binary sizes manageable.  
Mobile and Native UI Interoperability:  
Targeting mobile platforms (iOS and Android) presents a unique set of challenges and opportunities.

* **Initial Approach (Custom Rendering):** The most direct path is to treat mobile like any other platform. The wgpu backend can be compiled for Android and iOS, and the framework would render its entire UI into a single, full-screen native view (SurfaceView on Android, MTKView on iOS). This provides maximum cross-platform consistency, similar to Flutter.  
* **Advanced Interoperability (Long-Term Vision):** A more powerful and flexible long-term goal is to enable interoperability with native UI components, drawing inspiration from Compose Multiplatform for iOS.54 This would involve creating platform-specific backend crates that can translate a compose-rs UI tree into a native view hierarchy. For example, a compose\_rs\_ios crate could provide a function that takes a root \#\[composable\] function and returns a UIViewController that can be embedded within a larger SwiftUI or UIKit application.57 This "gradual adoption" path is critical for integration into existing enterprise applications. While some emerging Rust frameworks like WaterUI are exploring wrapping native widgets directly, this approach can be brittle, as it becomes dependent on the stability of the native platform's APIs.44 A custom-rendering approach provides greater control and consistency, making it the recommended primary strategy.

### **3.4 Enhancing Developer Experience (DX)**

A technically superior framework can fail without a strong focus on developer experience. For compose-rs to succeed, it must provide a productive and intuitive development environment.

IDE Integration:  
Deep integration with rust-analyzer is paramount. The \#\[composable\] macro must be designed to emit code that the language server can fully understand, preserving features like autocompletion, type inference, and "go to definition." This ensures that writing compose-rs code feels like writing any other idiomatic Rust code.  
Live Preview and Hot Reload:  
A defining feature of modern UI development is the ability to see changes instantly without a full recompile-and-relaunch cycle. compose-rs should provide a live preview tool. This could be implemented as a cargo subcommand or a standalone viewer application (similar to Slint's slint-viewer 38\) that monitors the source code. Upon a change, it would use Rust's dynamic linking capabilities to hot-reload the updated component library and refresh the UI, providing a rapid iteration loop for developers.  
Component Libraries:  
The layered architecture is designed to foster an ecosystem. The core framework would provide the foundation layer primitives. A first-party compose-rs-material crate would be developed to provide a complete Material Design component library, serving both as a useful toolkit for app developers and as a canonical example of how to build a design system on top of the foundation layer.6

## **Part IV: Strategic Recommendations and Future Outlook**

Building a comprehensive, cross-platform UI framework is a significant undertaking. A pragmatic, phased approach is required to manage complexity, build momentum, and deliver value incrementally. This final section outlines a strategic roadmap for the development of compose-rs, anticipates key challenges, and presents a concluding vision for the framework's potential impact on the software development landscape.

### **4.1 Phased Implementation Roadmap**

A disciplined, phased implementation is critical for success. Each phase should build upon the last, with clear goals and deliverables.

* **Phase 1: Core Runtime and Metaprogramming.** The initial focus must be entirely on the non-visual, architectural core. This includes:  
  * The design of the arena-allocated composition tree.  
  * The implementation of the \#\[composable\] procedural macro, focusing on the correct transformation of function signatures and the injection of runtime context.  
  * The creation of the fundamental state primitives: remember and state, including the underlying interior mutability patterns.  
  * The development of the recomposition scheduler.  
    This phase should be validated exclusively through a robust suite of unit and integration tests, without any graphical output.  
* **Phase 2: Layout and Rendering Abstraction.** With the core runtime in place, the next step is to define the bridge to the visual world. This involves:  
  * Designing and implementing the layout system. A Flexbox-inspired model, similar to that used by Jetpack Compose, is a proven and powerful choice.  
  * Defining a renderer-agnostic drawing trait. This API will abstract away the specifics of any graphics library, defining a set of commands like draw\_rect, draw\_text, etc.  
* **Phase 3: Initial Backends (Desktop and WASM).** This phase delivers the first visible output.  
  * Implement a rendering backend using wgpu to target desktop platforms.  
  * Implement a second backend using web-sys and wasm-bindgen to target WebAssembly, rendering to an HTML canvas.  
  * The deliverable for this phase is a "Hello, World\!" application that can be compiled and run on Windows, macOS, Linux, and in a web browser.  
* **Phase 4: Foundation Component Library.** A framework is unusable without a basic set of building blocks. This phase involves building the compose-rs-foundation crate, which will provide the design-system-agnostic components equivalent to Jetpack Compose's foundation layer.6 This includes:  
  * Layouts: Row, Column, Box, Stack.  
  * Text rendering and input.  
  * Basic image display.  
  * Scroll containers.  
  * Low-level gesture and input handling.  
* **Phase 5: Advanced Features and Mobile Exploration.** With a stable foundation, work can begin on more advanced features and platforms.  
  * Develop a high-level animation system.  
  * Implement more complex gesture detectors.  
  * Begin experimental work on the mobile backends for iOS and Android, focusing initially on full-screen custom rendering.  
  * Develop a first-party compose-rs-material library to showcase the framework's capabilities and encourage ecosystem growth.

### **4.2 Anticipated Challenges and Mitigation Strategies**

Several significant technical and strategic challenges must be addressed for compose-rs to succeed.

* **Borrow Checker Ergonomics:** This is the single greatest technical challenge. The APIs exposed to the end user must feel natural and ergonomic, hiding the inherent complexities of managing a stateful tree structure within Rust's ownership rules.  
  * **Mitigation:** The architecture must be designed *for* the borrow checker, not against it. Promoting a UDF and message-passing architecture is the primary mitigation, as it minimizes the need for shared mutable state. The core state primitives (state, remember) must carefully encapsulate any necessary interior mutability or Rc/Arc patterns, presenting a simple, safe interface to the developer. Extensive API design review and user testing will be critical.  
* **Macro Complexity and Compile Times:** Procedural macros are powerful but can be difficult to debug and can negatively impact Rust's already significant compile times.  
  * **Mitigation:** The \#\[composable\] macro should be kept as focused as possible, delegating most of its logic to runtime functions rather than generating vast amounts of code. The project should be heavily modularized into smaller crates to benefit from incremental compilation. Investment in build tooling and caching strategies will be necessary as the framework scales.  
* **Ecosystem Maturity:** A UI framework is only as valuable as the ecosystem of libraries and tools that surrounds it. Bootstrapping this ecosystem is a major challenge.  
  * **Mitigation:** The project must prioritize excellent documentation, tutorials, and example projects. Creating a high-quality, first-party Material Design library is essential to demonstrate the framework's power and provide a solid foundation for developers. Actively engaging with the community and making the framework easy to contribute to will be key to fostering a vibrant ecosystem.

### **4.3 Conclusion: The Vision for a Rust-Native Compose Framework**

The proposal for compose-rs should not be interpreted as a call for a mere line-by-line port of Jetpack Compose into Rust. Such an endeavor would fail, as it would inherit the architectural compromises made for the Kotlin/JVM environment without fully embracing the unique advantages of Rust.

Instead, the vision for compose-rs is a **reimagination of Compose's core principles through the lens of idiomatic Rust**. It is about taking the revolutionary ideas that make Compose a joy to use—a declarative component model, intelligent state-driven recomposition, and a flexible layered architecture—and re-implementing them on a foundation of Rust's unparalleled strengths.

The ultimate goal is to create a framework that delivers the developer experience and expressive power of Jetpack Compose, but with the performance, memory safety, and fearless concurrency that only Rust can provide. By leveraging procedural macros for ergonomic metaprogramming, the ownership model for a simplified and more robust performance system, and wgpu for modern, cross-platform rendering, compose-rs has the potential to become a best-in-class solution. It can provide a single, unified paradigm for building high-performance, native-quality user interfaces across every major platform: from resource-constrained embedded devices to powerful desktop workstations, from mobile applications to the web. This would not only be a significant contribution to the Rust ecosystem but would also represent a major step forward in the art of building user interface software.

#### **Works cited**

1. Jetpack Compose UI Architecture. Introduction | by Roman Levinzon \- Medium, accessed October 11, 2025, [https://levinzon-roman.medium.com/jetpack-compose-ui-architecture-a34c4d3e4391](https://levinzon-roman.medium.com/jetpack-compose-ui-architecture-a34c4d3e4391)  
2. State and Jetpack Compose | Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/state](https://developer.android.com/develop/ui/compose/state)  
3. Jetpack Compose UI App Development Toolkit \- Android Developers, accessed October 11, 2025, [https://developer.android.com/compose](https://developer.android.com/compose)  
4. Architecting your Compose UI | Jetpack Compose \- Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/architecture](https://developer.android.com/develop/ui/compose/architecture)  
5. Basic Overview of Google's Android UI Frameworks | by Dobri Kostadinov \- Medium, accessed October 11, 2025, [https://medium.com/@dobri.kostadinov/rendering-comparison-in-android-the-good-the-bad-and-the-ugly-jetpack-compose-flutter-and-907198b176e1](https://medium.com/@dobri.kostadinov/rendering-comparison-in-android-the-good-the-bad-and-the-ugly-jetpack-compose-flutter-and-907198b176e1)  
6. Jetpack Compose architectural layering | Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/layering](https://developer.android.com/develop/ui/compose/layering)  
7. KotlinConf 2019: The Compose Runtime, Demystified by Leland Richardson \- YouTube, accessed October 11, 2025, [https://www.youtube.com/watch?v=6BRlI5zfCCk](https://www.youtube.com/watch?v=6BRlI5zfCCk)  
8. Compose Compiler | Jetpack \- Android Developers, accessed October 11, 2025, [https://developer.android.com/jetpack/androidx/releases/compose-compiler](https://developer.android.com/jetpack/androidx/releases/compose-compiler)  
9. Lifecycle of composables | Jetpack Compose | Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/lifecycle](https://developer.android.com/develop/ui/compose/lifecycle)  
10. Android Jetpack Compose — Part 1 \- Medium, accessed October 11, 2025, [https://medium.com/@guruprasadhegde4/android-jetpack-compose-part-1-3a5b21092bbe](https://medium.com/@guruprasadhegde4/android-jetpack-compose-part-1-3a5b21092bbe)  
11. Thinking in Compose | Jetpack Compose \- Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/mental-model](https://developer.android.com/develop/ui/compose/mental-model)  
12. Gotchas in Jetpack Compose Recomposition \- MultiThreaded Technology at Stitch Fix, accessed October 11, 2025, [https://multithreaded.stitchfix.com/blog/2022/08/05/jetpack-compose-recomposition/](https://multithreaded.stitchfix.com/blog/2022/08/05/jetpack-compose-recomposition/)  
13. State Management in Android Jetpack Compose \- GeeksforGeeks, accessed October 11, 2025, [https://www.geeksforgeeks.org/android/state-management-in-android-jetpack-compose/](https://www.geeksforgeeks.org/android/state-management-in-android-jetpack-compose/)  
14. Jetpack Compose Recomposition. | by Abdullaherzincanli | Medium | Huawei Developers, accessed October 11, 2025, [https://medium.com/huawei-developers/jetpack-compose-recomposition-ebec29439560](https://medium.com/huawei-developers/jetpack-compose-recomposition-ebec29439560)  
15. Jetpack Compose State Management: A Guide for Android Developers | Bugfender, accessed October 11, 2025, [https://bugfender.com/blog/jetpack-compose-state-management/](https://bugfender.com/blog/jetpack-compose-state-management/)  
16. Reverse-Engineering the Compose Compiler Plugin: Intercepting the Frontend, accessed October 11, 2025, [https://hinchman-amanda.medium.com/reverse-engineering-the-compose-compiler-plugin-intercepting-the-frontend-657162893b11](https://hinchman-amanda.medium.com/reverse-engineering-the-compose-compiler-plugin-intercepting-the-frontend-657162893b11)  
17. Understanding Recomposition in Jetpack Compose | by Rizwanul Haque \- Stackademic, accessed October 11, 2025, [https://blog.stackademic.com/understanding-recomposition-in-jetpack-compose-0371a12c7fc2](https://blog.stackademic.com/understanding-recomposition-in-jetpack-compose-0371a12c7fc2)  
18. Compose compiler migration guide | Kotlin Documentation, accessed October 11, 2025, [https://kotlinlang.org/docs/compose-compiler-migration-guide.html](https://kotlinlang.org/docs/compose-compiler-migration-guide.html)  
19. Jetpack Compose compiler moving to the Kotlin repository \- Android Developers Blog, accessed October 11, 2025, [https://android-developers.googleblog.com/2024/04/jetpack-compose-compiler-moving-to-kotlin-repository.html](https://android-developers.googleblog.com/2024/04/jetpack-compose-compiler-moving-to-kotlin-repository.html)  
20. Updating Compose compiler | Kotlin Multiplatform Documentation \- JetBrains, accessed October 11, 2025, [https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-compiler.html](https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-compiler.html)  
21. Compose Compiler Gradle plugin \- Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/compiler](https://developer.android.com/develop/ui/compose/compiler)  
22. How to handle state in Jetpack Compose \- DECODE agency, accessed October 11, 2025, [https://decode.agency/article/jetpack-compose-state/](https://decode.agency/article/jetpack-compose-state/)  
23. The One Rule of State Management in Jetpack Compose You Can't Ignore | Medium, accessed October 11, 2025, [https://medium.com/@androidlab/the-one-rule-of-state-management-in-jetpack-compose-you-cant-ignore-8e50739586cf](https://medium.com/@androidlab/the-one-rule-of-state-management-in-jetpack-compose-you-cant-ignore-8e50739586cf)  
24. Mastering Jetpack Compose: Best Practices and Architectural Patterns \- Medium, accessed October 11, 2025, [https://medium.com/make-android/mastering-jetpack-compose-best-practices-and-architectural-patterns-89fdfe837772](https://medium.com/make-android/mastering-jetpack-compose-best-practices-and-architectural-patterns-89fdfe837772)  
25. State Management Patterns in Jetpack Compose \- Carrion.dev, accessed October 11, 2025, [https://carrion.dev/en/posts/state-management-patterns-compose/](https://carrion.dev/en/posts/state-management-patterns-compose/)  
26. im-o/jetpack-compose-clean-architecture: Modularized Jetpack Compose App with Use Case Pattern | App Marketplace Sample \- GitHub, accessed October 11, 2025, [https://github.com/im-o/jetpack-compose-clean-architecture](https://github.com/im-o/jetpack-compose-clean-architecture)  
27. State Management in Jetpack Compose: ViewModel vs. Remember Function \- Stackademic, accessed October 11, 2025, [https://blog.stackademic.com/state-management-in-jetpack-compose-viewmodel-vs-remember-function-5fc78cdec92f](https://blog.stackademic.com/state-management-in-jetpack-compose-viewmodel-vs-remember-function-5fc78cdec92f)  
28. Kotlin native vs Rust, accessed October 11, 2025, [https://discuss.kotlinlang.org/t/kotlin-native-vs-rust/9785](https://discuss.kotlinlang.org/t/kotlin-native-vs-rust/9785)  
29. Battle Of The Backends: Rust vs. Go vs. C\# vs. Kotlin \- inovex GmbH, accessed October 11, 2025, [https://www.inovex.de/de/blog/rust-vs-go-vs-c-vs-kotlin/](https://www.inovex.de/de/blog/rust-vs-go-vs-c-vs-kotlin/)  
30. Kotlin or Rust? Which language has fewer syntax and runtime errors? \- Quora, accessed October 11, 2025, [https://www.quora.com/Which-programming-language-is-better-for-learning-Kotlin-or-Rust-Which-language-has-fewer-syntax-and-runtime-errors](https://www.quora.com/Which-programming-language-is-better-for-learning-Kotlin-or-Rust-Which-language-has-fewer-syntax-and-runtime-errors)  
31. Rust vs Kotlin: Key Differences \- GeeksforGeeks, accessed October 11, 2025, [https://www.geeksforgeeks.org/blogs/rust-vs-kotlin/](https://www.geeksforgeeks.org/blogs/rust-vs-kotlin/)  
32. For Complex Applications, Rust is as Productive as Kotlin \- Ferrous Systems, accessed October 11, 2025, [https://ferrous-systems.com/blog/rust-as-productive-as-kotlin/](https://ferrous-systems.com/blog/rust-as-productive-as-kotlin/)  
33. So I tried Rust for the first time. \- DEV Community, accessed October 11, 2025, [https://dev.to/martinhaeusler/so-i-tried-rust-for-the-first-time-4jdb](https://dev.to/martinhaeusler/so-i-tried-rust-for-the-first-time-4jdb)  
34. Advice for the next dozen Rust GUIs | Raph Levien's blog, accessed October 11, 2025, [https://raphlinus.github.io/rust/gui/2022/07/15/next-dozen-guis.html](https://raphlinus.github.io/rust/gui/2022/07/15/next-dozen-guis.html)  
35. Are we GUI yet?, accessed October 11, 2025, [https://areweguiyet.com/](https://areweguiyet.com/)  
36. Architecture \- iced — A Cross-Platform GUI Library for Rust, accessed October 11, 2025, [https://book.iced.rs/architecture.html](https://book.iced.rs/architecture.html)  
37. iced-rs/iced: A cross-platform GUI library for Rust, inspired by Elm \- GitHub, accessed October 11, 2025, [https://github.com/iced-rs/iced](https://github.com/iced-rs/iced)  
38. Slint is an open-source declarative GUI toolkit to build native user interfaces for Rust, C++, JavaScript, or Python apps. \- GitHub, accessed October 11, 2025, [https://github.com/slint-ui/slint](https://github.com/slint-ui/slint)  
39. Slint | Declarative GUI for Rust, C++, JavaScript & Python, accessed October 11, 2025, [https://slint.dev/](https://slint.dev/)  
40. How do popular Rust UI libraries compare? Iced vs Slint vs Egui \- Reddit, accessed October 11, 2025, [https://www.reddit.com/r/rust/comments/1iavpit/how\_do\_popular\_rust\_ui\_libraries\_compare\_iced\_vs/](https://www.reddit.com/r/rust/comments/1iavpit/how_do_popular_rust_ui_libraries_compare_iced_vs/)  
41. Top GUI Libraries and Frameworks for Rust A Comprehensive Guide, accessed October 11, 2025, [https://simplifycpp.org/?id=a0507](https://simplifycpp.org/?id=a0507)  
42. linebender/druid: A data-first Rust-native UI design toolkit. \- GitHub, accessed October 11, 2025, [https://github.com/linebender/druid](https://github.com/linebender/druid)  
43. Overview \- Druid \- Linebender, accessed October 11, 2025, [https://linebender.org/druid/01\_overview.html](https://linebender.org/druid/01_overview.html)  
44. WaterUI: A SwiftUI-inspired cross-platform UI framework for Rust with cross-platform native rendering \- Reddit, accessed October 11, 2025, [https://www.reddit.com/r/rust/comments/1nauo67/waterui\_a\_swiftuiinspired\_crossplatform\_ui/](https://www.reddit.com/r/rust/comments/1nauo67/waterui_a_swiftuiinspired_crossplatform_ui/)  
45. WaterUI: A SwiftUI-inspired cross-platform UI framework for Rust with cross-platform native rendering \- announcements \- The Rust Programming Language Forum, accessed October 11, 2025, [https://users.rust-lang.org/t/waterui-a-swiftui-inspired-cross-platform-ui-framework-for-rust-with-cross-platform-native-rendering/133717](https://users.rust-lang.org/t/waterui-a-swiftui-inspired-cross-platform-ui-framework-for-rust-with-cross-platform-native-rendering/133717)  
46. Writing a native GUI app in Rust with Iced \- Broch Web Solutions, accessed October 11, 2025, [https://www.brochweb.com/blog/post/writing-a-native-gui-app-in-rust-with-iced/](https://www.brochweb.com/blog/post/writing-a-native-gui-app-in-rust-with-iced/)  
47. iced \- A cross-platform GUI library for Rust, accessed October 11, 2025, [https://iced.rs/](https://iced.rs/)  
48. Comparison of GUI libraries in February 2024 : r/rust \- Reddit, accessed October 11, 2025, [https://www.reddit.com/r/rust/comments/1avzrnz/comparison\_of\_gui\_libraries\_in\_february\_2024/](https://www.reddit.com/r/rust/comments/1avzrnz/comparison_of_gui_libraries_in_february_2024/)  
49. Best UI framework? : r/rust \- Reddit, accessed October 11, 2025, [https://www.reddit.com/r/rust/comments/1fxtrvk/best\_ui\_framework/](https://www.reddit.com/r/rust/comments/1fxtrvk/best_ui_framework/)  
50. Jetpack Compose phases \- Android Developers, accessed October 11, 2025, [https://developer.android.com/develop/ui/compose/phases](https://developer.android.com/develop/ui/compose/phases)  
51. Compiling from Rust to WebAssembly \- MDN \- Mozilla, accessed October 11, 2025, [https://developer.mozilla.org/en-US/docs/WebAssembly/Guides/Rust\_to\_Wasm](https://developer.mozilla.org/en-US/docs/WebAssembly/Guides/Rust_to_Wasm)  
52. WebAssembly \- Rust Programming Language, accessed October 11, 2025, [https://rust-lang.org/what/wasm/](https://rust-lang.org/what/wasm/)  
53. Is Rust/WASM based web-UI worthwhile for smaller projects? \- community, accessed October 11, 2025, [https://users.rust-lang.org/t/is-rust-wasm-based-web-ui-worthwhile-for-smaller-projects/73075](https://users.rust-lang.org/t/is-rust-wasm-based-web-ui-worthwhile-for-smaller-projects/73075)  
54. Create your Compose Multiplatform app \- JetBrains, accessed October 11, 2025, [https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-multiplatform-create-first-app.html](https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-multiplatform-create-first-app.html)  
55. Compose Multiplatform – Beautiful UIs Everywhere \- JetBrains, accessed October 11, 2025, [https://www.jetbrains.com/compose-multiplatform/](https://www.jetbrains.com/compose-multiplatform/)  
56. Integration with the SwiftUI framework | Kotlin Multiplatform Documentation \- JetBrains, accessed October 11, 2025, [https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-swiftui-integration.html](https://www.jetbrains.com/help/kotlin-multiplatform-dev/compose-swiftui-integration.html)  
57. Why Your Compose Multiplatform App Still Needs Native Code | by Suhyeon Kim | Oct, 2025, accessed October 11, 2025, [https://proandroiddev.com/why-your-compose-multiplatform-app-still-needs-native-code-a7e56bffeaea](https://proandroiddev.com/why-your-compose-multiplatform-app-still-needs-native-code-a7e56bffeaea)  
58. Kotlin Multiplatform – Bridging Compose & iOS UI Frameworks | Infinum, accessed October 11, 2025, [https://infinum.com/blog/kotlin-multiplatform-swiftui/](https://infinum.com/blog/kotlin-multiplatform-swiftui/)