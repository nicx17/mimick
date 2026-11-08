//! Values Immich has for a search field, from `GET /search/suggestions`: countries, states,
//! cities, camera makes, models, and lenses.

use super::ImmichApiClient;

/// Which field to suggest values for, and the values chosen above it that narrow the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionField {
    Country,
    State,
    City,
    CameraMake,
    CameraModel,
    LensModel,
}

impl SuggestionField {
    fn name(self) -> &'static str {
        match self {
            SuggestionField::Country => "country",
            SuggestionField::State => "state",
            SuggestionField::City => "city",
            SuggestionField::CameraMake => "camera-make",
            SuggestionField::CameraModel => "camera-model",
            SuggestionField::LensModel => "camera-lens-model",
        }
    }
}

/// Values already chosen; Immich narrows models by make and cities by country and state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SuggestionContext {
    pub country: Option<String>,
    pub state: Option<String>,
    pub make: Option<String>,
    pub model: Option<String>,
}

fn suggestion_query(field: SuggestionField, context: &SuggestionContext) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("type", field.name());
    let narrowing = [
        ("country", &context.country),
        ("state", &context.state),
        ("make", &context.make),
        ("model", &context.model),
    ];
    for (name, value) in narrowing {
        if let Some(value) = value {
            query.append_pair(name, value);
        }
    }
    query.finish()
}

impl ImmichApiClient {
    /// The values Immich has for `field`, narrowed by `context`, without empty ones.
    pub async fn fetch_search_suggestions(
        &self,
        field: SuggestionField,
        context: &SuggestionContext,
    ) -> Result<Vec<String>, String> {
        let path = format!(
            "/api/search/suggestions?{}",
            suggestion_query(field, context)
        );
        let resp = self.send_json(reqwest::Method::GET, &path, None).await?;
        let values: Vec<Option<String>> = resp.json().await.map_err(|err| err.to_string())?;
        Ok(values
            .into_iter()
            .flatten()
            .filter(|value| !value.trim().is_empty())
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_are_narrowed_by_the_fields_chosen() {
        let context = SuggestionContext {
            make: Some("Canon".into()),
            ..SuggestionContext::default()
        };
        assert_eq!(
            suggestion_query(SuggestionField::CameraModel, &context),
            "type=camera-model&make=Canon"
        );
        assert_eq!(
            suggestion_query(SuggestionField::Country, &SuggestionContext::default()),
            "type=country"
        );
    }
}
