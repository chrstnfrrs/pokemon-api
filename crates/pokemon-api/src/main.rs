use aws_lambda_events::{
    encodings::Body,
    event::apigw::{ApiGatewayProxyRequest, ApiGatewayProxyResponse},
};
use http::header::HeaderMap;
use lambda_runtime::{service_fn, Error, LambdaEvent};
use once_cell::sync::OnceCell;
use serde::Serialize;
use serde_json::json;
use sqlx::{mysql::MySqlPoolOptions, MySql, Pool};
use std::env;
use tracing::{error, info, instrument};
use upload_pokemon_data::PokemonId;

static POOL: OnceCell<Pool<MySql>> = OnceCell::new();

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt::init();
    let database_url = env::var("DATABASE_URL")?;
    let pool = MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;
    POOL.get_or_init(|| pool);
    let processor = service_fn(handler);
    lambda_runtime::run(processor).await?;
    Ok(())
}

#[derive(Debug, sqlx::FromRow, Serialize)]
struct PokemonHp {
    id: PokemonId,
    name: String,
    hp: u16,
    legendary_or_mythical: bool,
}

#[instrument]
async fn handler(
    event: LambdaEvent<ApiGatewayProxyRequest>,
) -> Result<ApiGatewayProxyResponse, Error> {
    let (event, _context) = event.into_parts();

    let path = event.path.expect("expect there to always be an event path");
    let requested_pokemon = path.split("/").last();
    match requested_pokemon {
        Some("") => {
            error!("searched for empty pokemon");
            let error_message = serde_json::to_string(&json!({
                "error": "searched for empty pokemon"
            }))?;
            let response = ApiGatewayProxyResponse {
                status_code: 400,
                headers: HeaderMap::new(),
                multi_value_headers: HeaderMap::new(),
                body: Some(Body::Text(error_message)),
                is_base64_encoded: Some(false),
            };
            Ok(response)
        }
        None => panic!("requested_pokemon is None, which should never happen"),
        Some(pokemon_name) => {
            info!(pokemon_name, "requested a pokemon");
            let result = sqlx::query_as!(
                PokemonHp,
                r#"
SELECT 
    id as "id!: PokemonId",
    name as "name!: String",
    hp,
    legendary_or_mythical as "legendary_or_mythical!: bool"
FROM
    pokemon
WHERE
    slug = ?
"#,
                pokemon_name
            )
            .fetch_one(POOL.get().unwrap())
            .await?;

            let json_pokemon = serde_json::to_string(&result)?;
            let response = ApiGatewayProxyResponse {
                status_code: 200,
                headers: HeaderMap::new(),
                multi_value_headers: HeaderMap::new(),
                body: Some(Body::Text(json_pokemon)),
                is_base64_encoded: Some(false),
            };
            Ok(response)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use aws_lambda_events::event::apigw::{
        ApiGatewayProxyRequestContext, ApiGatewayRequestIdentity,
    };
    use http::Method;
    use lambda_runtime::Context;

    use super::*;

    #[tokio::test]
    async fn handler_handles() {
        let database_url = env::var("DATABASE_URL").unwrap();
        let pool = MySqlPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
            .unwrap();
        POOL.get_or_init(|| pool);

        let event = fake_request("/api/pokemon/bulbasaur".to_string());

        let fake_id = PokemonId::new();

        assert_eq!(
            handler(LambdaEvent::new(event, Context::default()))
                .await
                .unwrap(),
            ApiGatewayProxyResponse {
                status_code: 200,
                headers: HeaderMap::new(),
                multi_value_headers: HeaderMap::new(),
                body: Some(Body::Text(
                    serde_json::to_string(&PokemonHp {
                        id: fake_id,
                        name: String::from("Bulbasaur"),
                        hp: 45,
                        legendary_or_mythical: false
                    },)
                    .unwrap()
                )),
                is_base64_encoded: Some(false),
            }
        )
    }

    #[tokio::test]
    async fn handler_handles_empty_pokemon() {
        let event = fake_request("/api/pokemon//".to_string());
        assert_eq!(
            handler(LambdaEvent::new(event, Context::default()))
                .await
                .unwrap(),
            ApiGatewayProxyResponse {
                status_code: 400,
                headers: HeaderMap::new(),
                multi_value_headers: HeaderMap::new(),
                body: Some(Body::Text(
                    serde_json::to_string(&json!({
                        "error": "searched for empty pokemon"
                    }))
                    .unwrap()
                )),
                is_base64_encoded: Some(false),
            }
        );
    }

    fn fake_request(path: String) -> ApiGatewayProxyRequest {
        ApiGatewayProxyRequest {
            resource: None,
            path: Some(path),
            http_method: Method::GET,
            headers: HeaderMap::new(),
            multi_value_headers: HeaderMap::new(),
            query_string_parameters: Default::default(),
            multi_value_query_string_parameters: Default::default(),
            path_parameters: HashMap::new(),
            stage_variables: HashMap::new(),
            request_context: ApiGatewayProxyRequestContext {
                account_id: None,
                resource_id: None,
                operation_name: None,
                stage: None,
                domain_name: None,
                domain_prefix: None,
                request_id: None,
                protocol: None,
                identity: ApiGatewayRequestIdentity {
                    cognito_identity_pool_id: None,
                    account_id: None,
                    cognito_identity_id: None,
                    caller: None,
                    api_key: None,
                    api_key_id: None,
                    access_key: None,
                    source_ip: None,
                    cognito_authentication_type: None,
                    cognito_authentication_provider: None,
                    user_arn: None,
                    user_agent: None,
                    user: None,
                },
                resource_path: None,
                authorizer: HashMap::new(),
                http_method: Method::GET,
                request_time: None,
                request_time_epoch: 0,
                apiid: None,
            },
            body: None,
            is_base64_encoded: Some(false),
        }
    }
}
