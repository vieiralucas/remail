use dioxus::prelude::*;
mod api;

use api::ApiClient;
use remail_types::Email;

fn format_subject(subject: &Option<String>) -> &str {
    subject.as_deref().unwrap_or("(no subject)")
}

fn format_date(datetime: &chrono::DateTime<chrono::Utc>) -> String {
    datetime.format("%Y-%m-%d %H:%M").to_string()
}

#[derive(Debug, Clone, Routable, PartialEq)]
#[rustfmt::skip]
enum Route {
    #[route("/")]
    Home {},
}

const FAVICON: Asset = asset!("/assets/favicon.ico");
const MAIN_CSS: Asset = asset!("/assets/main.css");
const TAILWIND_CSS: Asset = asset!("/assets/tailwind.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    rsx! {
        document::Link { rel: "icon", href: FAVICON }
        document::Link { rel: "stylesheet", href: MAIN_CSS } document::Link { rel: "stylesheet", href: TAILWIND_CSS }
        Router::<Route> {}
    }
}

/// Home page
#[component]
fn Home() -> Element {
    let emails = use_signal(Vec::<Email>::new);
    let loading = use_signal(|| false);
    let error = use_signal(|| Option::<String>::None);

    use_effect(move || {
        let mut emails = emails;
        let mut loading = loading;
        let mut error = error;

        spawn(async move {
            loading.set(true);
            error.set(None);

            let client = ApiClient::new();
            match client.list_emails().await {
                Ok(emails_data) => {
                    emails.set(emails_data);
                }
                Err(e) => {
                    error.set(Some(format!("Failed to load emails: {e}")));
                }
            }
            loading.set(false);
        });
    });

    rsx! {
        div {
            class: "container mx-auto px-4 py-8",
            h1 {
                class: "text-3xl font-bold mb-8",
                "Email List"
            }

            if loading() {
                div {
                    class: "text-center py-8",
                    "Loading emails..."
                }
            } else if let Some(err) = error() {
                div {
                    class: "bg-red-100 border border-red-400 text-red-700 px-4 py-3 rounded mb-4",
                    "Error: {err}"
                }
            } else {
                div {
                    class: "space-y-4",
                    for email in emails().iter() {
                        div {
                            class: "bg-white border border-gray-200 rounded-lg p-6 shadow-sm",
                            div {
                                class: "flex justify-between items-start mb-2",
                                h2 {
                                    class: "text-xl font-semibold text-gray-900",
                                    "{format_subject(&email.subject)}"
                                }
                                span {
                                    class: "text-sm text-gray-500",
                                    "{format_date(&email.created_at)}"
                                }
                            }
                            div {
                                class: "text-sm text-gray-600 mb-2",
                                "From: {email.from}"
                            }
                            div {
                                class: "text-sm text-gray-600 mb-3",
                                "To: {email.to}"
                            }
                            div {
                                class: "text-gray-700 line-clamp-3",
                                "{email.body}"
                            }
                        }
                    }
                }
            }
        }
    }
}
