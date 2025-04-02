use dioxus::prelude::*;
use dioxus_html_macro::html;

use crate::components::{MainContent, Page, Section, SectionLevel};

#[component]
pub fn RoadmapPage() -> Element {
    rsx! {
        Page { title: "Roadmap".into(),
            h1 { "Roadmap" }
            div {
                dangerous_inner_html: r#"
                    <p>In general our current priority is to get the renderer "up to scratch" for two use cases:</p>
                    <ul>
                        <li>HTML/CSS webpage rendering</li>
                        <li>Application runtime (Dioxus Native)</li>
                    </ul>
                "#,
            }
            MainContent {
                Section {
                    heading: "Milestones",
                    level: SectionLevel::H2,

                    Section {
                        heading: "Pre-Alpha M1 (June 2025)",
                        level: SectionLevel::H4,

                        div {
                            dangerous_inner_html: r#"
                            <ul>
                                <li><b>Initial crates.io releases</b> - Initial versions of </li>
                                <li><b>WGPU texture rendering</b> (<code>&lt;canvas&gt;</code> support) - </li>
                                <li><b>calc()</b> - Support for calc() values in CSS styles.</li>
                            </ul>
                            "#
                        }
                    }

                    Section {
                        heading: "Pre-Alpha M2 (Sept 2025)",
                        level: SectionLevel::H4,

                        div {
                            dangerous_inner_html: r#"
                            <ul>
                                <li><b>Layout: Named Grid Lines and Areas</b> - Initial versions of </li>
                                <li><b>Testing: Run WPT tests on every PR</b> - </li>
                                <li><b>Devtools: Layout inspector</b> - </li>
                            </ul>
                            "#
                        }
                    }

                    Section {
                        heading: "Alpha M3 (Dec 2025)",
                        level: SectionLevel::H4,

                    }
                }


                Section {
                    heading: "Backlog",
                    level: SectionLevel::H2,

                    Section {
                        heading: "Layout",
                        level: SectionLevel::H3,
                        div {
                            Section {
                                heading: "CSS Grid: named lines and areas",
                                level: SectionLevel::H4,
                                div {
                                    dangerous_inner_html: r#"
                                        <p>Another feature which commonly affects the top-level layout of pages and causes the layout to be completely broken
                                        when not implemented. This one is relatively straightforward to implement as it mostly as "syntax sugar" over regular
                                        grid positioning (using numbered row/column positions).</p>
                                    "#,
                                }
                            }
                            Section {
                                heading: "Floats",
                                level: SectionLevel::H4,
                                div {
                                    dangerous_inner_html: r#"
                                        <p>Floats are a legacy part of the web platform, but an important one with ~77% websites containing at least
                                        one float. Furthermore, floats are still widely used for general layout purposes which means that attempting to
                                        render website which use floats without supporting them can lead to website with completely broken layouts. This
                                        effects major website like wikipedia.org and old.reddit.com an we have identified this as one of the main 
                                        things preventing us from rendering "most websites" correctly.</p>

                                        <p>Floats are complex to implement breaking the clean layering between box-level and text/inline-level layout (which are
                                        separate libraries in Blitz!), but we have a plan in place.</p>
                                    "#,
                                }
                            }

                        }
                    }

                    Section {
                        heading: "Event handling improvements",
                        level: SectionLevel::H3,
                        div {
                            dangerous_inner_html: r#"
                                <p>Blitz does not currently support JavaScript, but it does support scripting / interactivity via it's
                                dioxus-native integration. However, support is currently somewhat immature, both in terms of the number of events
                                supported (currently a limited selection) and in terms of correctness/robustness.</p>

                                <p>Building this out will support the Dioxus Native (/application runtime) use case, and will also put us in a strong
                                position if/when we want to start building out JavaScript support in future.</p>
                            "#,
                        }
                    }

                    Section {
                        heading: "Debugging and Testing",
                        level: SectionLevel::H3,
                        Section {
                            heading: "Devtool support",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                        Section {
                            heading: "WPT Testing",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                    }

                    Section {
                        heading: "HTML features",
                        level: SectionLevel::H3,
                        Section {
                            heading: "Details/Summary elements",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                        Section {
                            heading: "Popover API",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                        Section {
                            heading: "Form controls",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                        Section {
                            heading: "Canvas (WGPU)",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                    }

                    Section {
                        heading: "Browser UI",
                        level: SectionLevel::H3,
                        div {
                            dangerous_inner_html: r#"
                                <p>lorem ipsum</p>
                            "#,
                        }
                    }

                    Section {
                        heading: "Other",
                        level: SectionLevel::H3,
                        Section {
                            heading: "Incremental \"tree construction\"",
                            level: SectionLevel::H4,
                            div {
                                dangerous_inner_html: r#"
                                    <p>lorem ipsum</p>
                                "#,
                            }
                        }
                    }
                }
            }
        }
    }
}