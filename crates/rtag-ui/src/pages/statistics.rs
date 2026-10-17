//! A project's statistics: annotation counts per label, per job and per user.

use leptos::prelude::*;
use rtag_core::job::Statistics;

use crate::api;

/// "3 / 10" with a bar: annotated images of all.
pub fn progress(annotated: i64, images: i64) -> impl IntoView {
    let percent = if images > 0 { annotated * 100 / images } else { 0 };
    view! {
        <td>
            <span class="progress">
                <span class="mono">{format!("{annotated} / {images}")}</span>
                <span class="bar"><span style:width=format!("{percent}%")></span></span>
            </span>
        </td>
    }
}

#[component]
pub fn StatisticsTab(project: i64) -> impl IntoView {
    let statistics = LocalResource::new(move || async move { api::get::<Statistics>(&format!("/api/projects/{project}/statistics")).await });
    move || match statistics.get() {
        None => view! { <p class="muted">"Loading…"</p> }.into_any(),
        Some(Err(message)) => view! { <p class="error" role="alert">{message}</p> }.into_any(),
        Some(Ok(Statistics { labels, jobs, users })) => view! {
            <div class="statistics">
                <section>
                    <h2>"Per label"</h2>
                    <table class="labels per-label">
                        <thead><tr><th>"Label"</th><th>"Annotations"</th><th>"Images"</th></tr></thead>
                        <tbody>
                            {labels.into_iter().map(|label| view! {
                                <tr>
                                    <td><span class="swatch" style:background=label.color></span>{label.name}</td>
                                    <td class="mono">{label.annotations}</td>
                                    <td class="mono">{label.images}</td>
                                </tr>
                            }).collect_view()}
                        </tbody>
                    </table>
                </section>
                <section>
                    <h2>"Per job"</h2>
                    {if jobs.is_empty() {
                        view! { <p class="muted">"No jobs yet."</p> }.into_any()
                    } else {
                        view! {
                            <table class="labels per-job">
                                <thead><tr><th>"Job"</th><th>"Assigned to"</th><th>"Annotated"</th><th>"Annotations"</th></tr></thead>
                                <tbody>
                                    {jobs.into_iter().map(|job| view! {
                                        <tr>
                                            <td>{format!("Job {}", job.number)}</td>
                                            <td>{job.assignee.map(|user| user.username).unwrap_or_else(|| "Nobody".to_owned())}</td>
                                            {progress(job.annotated, job.images)}
                                            <td class="mono">{job.annotations}</td>
                                        </tr>
                                    }).collect_view()}
                                </tbody>
                            </table>
                        }.into_any()
                    }}
                </section>
                <section>
                    <h2>"Per user"</h2>
                    <p class="muted hint">"Counts the images of the jobs assigned to each user now."</p>
                    <table class="labels per-user">
                        <thead><tr><th>"User"</th><th>"Annotated"</th><th>"Annotations"</th></tr></thead>
                        <tbody>
                            {users.into_iter().map(|row| view! {
                                <tr>
                                    <td>{row.user.map(|user| user.username).unwrap_or_else(|| "No job or unassigned".to_owned())}</td>
                                    {progress(row.annotated, row.images)}
                                    <td class="mono">{row.annotations}</td>
                                </tr>
                            }).collect_view()}
                        </tbody>
                    </table>
                </section>
            </div>
        }.into_any(),
    }
}
