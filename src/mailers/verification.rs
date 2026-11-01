use anyhow::{Result, anyhow};
use async_trait::async_trait;
use loco_rs::app::AppContext;
use loco_rs::mailer::{Email, Mailer};
use tera::Context;

struct VerificationMailer;

#[async_trait]
impl Mailer for VerificationMailer {}

/// Sends a verification email to a newly registered user.
/// Uses the Loco mailer system if configured, falls back to logging.
pub async fn send_verification_email(
    ctx: &AppContext,
    to: &str,
    username: &str,
    token: &str,
    base_url: &str,
) -> Result<()> {
    let verify_url = format!(
        "{}/verify-email?token={}",
        base_url.trim_end_matches('/'),
        token
    );

    let tera = tera::Tera::default();
    let mut context = Context::new();
    context.insert("username", username);
    context.insert("verify_url", &verify_url);

    let subject = "Verify your Taverna account".to_string();
    let html = tera.render_str(
        r#"<!DOCTYPE html>
<html>
<head>
  <meta charset="UTF-8">
  <title>Verify your Taverna account</title>
</head>
<body style="font-family: sans-serif; max-width: 600px; margin: 0 auto; padding: 20px;">
  <h1>Welcome to Taverna, {{ username }}!</h1>
  <p>Please verify your email address by clicking the link below:</p>
  <p><a href="{{ verify_url }}">{{ verify_url }}</a></p>
  <p>This link expires in 24 hours.</p>
  <p>If you did not create this account, please ignore this email.</p>
</body>
</html>"#,
        &context,
        true,
    )?;
    let text = tera.render_str(
        "Welcome to Taverna, {{ username }}!\n\nPlease verify your email address by visiting the link below:\n\n{{ verify_url }}\n\nThis link expires in 24 hours.\n\nIf you did not create this account, please ignore this email.",
        &context,
        false,
    )?;

    if ctx.mailer.is_some() {
        VerificationMailer::mail(
            ctx,
            &Email {
                to: to.to_string(),
                subject,
                text,
                html,
                ..Default::default()
            },
        )
        .await
        .map_err(|e| anyhow!("Failed to queue verification email: {}", e))?;
    } else {
        tracing::info!(
            "[mailer] Verification email for {} <{}>: {}",
            username,
            to,
            verify_url
        );
    }

    Ok(())
}
