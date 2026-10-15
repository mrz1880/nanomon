#[cfg(test)]
mod api_contract_tests;
mod assets;
mod handlers;
mod routes;

pub use routes::create_router;
