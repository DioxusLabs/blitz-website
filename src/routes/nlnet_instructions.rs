use dioxus::prelude::*;

use crate::components::AnchorHeader;
use crate::components::Page;
use crate::components::SectionLevel::*;

#[component]
pub fn NLNetInstructionsPage() -> Element {
    rsx! {
        Page {
            title: "NLNet Testing Instructions".into(),

            h1 { "NLNet Testing Instructions" }

            p {
                dangerous_inner_html: r#"
                    This page is meant to help people working at NLnet to test Blitz in order to verify that work completed as part of the NLnet grant
                    to Blitz has in fact been completed. It may also be useful to anyone else wishing to test Blitz.
                "#
            }

            AnchorHeader {
                level: H2,
                target: "pre-requisites",
                "Pre-requisites"
            }

            p {
                "You will need the following to build and test Blitz:"
            }

            ul {
                li {
                    dangerous_inner_html: r#"
                        <b>The Blitz source code</b>. The code is hosted on GitHub at <a href="https://github.com/DioxusLabs/blitz">https://github.com/DioxusLabs/blitz</a>
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>A Rust toolchain</b>. Follow the instructions at <a href="https://rustup.rs">https://rustup.rs</a> to install
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>Python 3</b> needs to be installed and available in $PATH. This is available by default on macOS and Linux but may need to be installed on Windows.
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>The "just" task runner</b> (pptional, but recommended). <a href="https://github.com/casey/just">https://github.com/casey/just</a>) to run commands.It can be installed with "cargo install just" (or by various other methods described in the just project's README). Just commands are basically alises (with parameters) to avoid repeatedly writing out long commands, so if you do not wish to install "just" then you can peek into the file named "justfile" in the Blitz repo to see to expanded command you need to run.
                    "#
                }
            }

            AnchorHeader {
                level: H2,
                target: "baseline-build",
                "Baseline build (for comparison)"
            }

            p {
                dangerous_inner_html: r#"
                    I have prepared a "nlnet-baseline" branch for you to compare latest main against. This is based on commit <a href="https://github.com/DioxusLabs/blitz/commit/4bc0efe947bd6e37352dd0b8be247bb9612942d5">4bc0efe947bd6e37352dd0b8be247bb9612942d5</a> from the 2025-03-21, the day I was notified that my NLnet grant was officially approved. I have then backported a couple of fixes and convenience commands to make it easier to test.
                "#
            }

            ul {
                li {
                    p {
                        dangerous_inner_html: r#"
                            To build a "blitz-baseline" binary into ./target/release/blitz-baseline, run the following:
                        "#
                    }
                    ul {
                        li {
                            dangerous_inner_html: r#"
                                git checkout nlnet-baseline
                            "#
                        }
                        li {
                            dangerous_inner_html: r#"
                                just build
                            "#
                        }
                    }
                }
                li {
                    dangerous_inner_html: r#"
                        It can be run with ./target/release/blitz-baseline <url>
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        This binary is standalone and can be freely moved to anywhere else on your filesystem. I recommend moving/copying to somewhere where it won't get clobbered by subsequent builds (perhaps somewhere in your $PATH so you can run it with just 'blitz-baseline <url>"
                    "#
                }
            }


            AnchorHeader {
                level: H2,
                target: "recent-builds",
                "Building recent versions"
            }

            p {
                "You can test with the latest version of the main branch: git checkout main"
            }

            p {
                "The following commands can be run from the repo root:"
            }

            ul {
                li {
                    dangerous_inner_html: r#"
                        <b>"just browser"</b> opens the new browser ui (with incremental construction and http cache enabled)
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>"just browser-with-perf"</b> is like "just browser" and additional enables logging performance logs to the console.
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>"just incr URL"</b> opens a url with no browser UI and incremental construction enabled (but no http cache)
                    "#
                }
                li {
                    dangerous_inner_html: r#"
                        <b>"just open URL"</b> opens a url with no browser UI and incremental construction disabled (and no http cache)
                    "#
                }
            }

            p {
                "It probably makes sense to test with the browser UI, except when testing with incremental mode disabled (just open URL)"
            }

            p {
                "The browser UI built with these commands is *not* a standalone binary and will not work properly if moved. A standalone app bundle can be built using 'dx bundle -p browser' where 'dx' is the Dioxus CLI (cargo install -f dioxus-cli)."
            }


        }
    }
}
