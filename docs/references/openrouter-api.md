Using the OpenRouter API
The most direct way to use OpenRouter. Send standard HTTP requests to the /api/v1/chat/completions endpoint — compatible with any language or framework.
You can use the interactive Request Builder to generate OpenRouter API requests in the language of your choice.
The examples below use ~openai/gpt-latest, a latest alias that always resolves to the newest OpenAI flagship model — so your code keeps using the freshest version without redeploying. You can substitute any model slug here. Browse the full catalog at openrouter.ai/models, or list every available slug programmatically via the GET /api/v1/models endpoint.

import requests
import json

response = requests.post(
  url="https://openrouter.ai/api/v1/chat/completions",
  headers={
    "Authorization": "Bearer <OPENROUTER_API_KEY>",
    "HTTP-Referer": "<YOUR_SITE_URL>", # Optional. Site URL for rankings on openrouter.ai.
    "X-OpenRouter-Title": "<YOUR_SITE_NAME>", # Optional. Site title for rankings on openrouter.ai.
  },
  data=json.dumps({
    "model": "~openai/gpt-latest",
    "messages": [
      {
        "role": "user",
        "content": "What is the meaning of life?"
      }
    ]
  })
)

The API also supports streaming. You can also use the OpenAI SDK pointed at OpenRouter as a drop-in replacement.

Models

One API for hundreds of models
Explore and browse 400+ models and providers on our website, or with our API. You can also subscribe to our RSS feed to stay updated on new models.
​
Query Parameters
The Models API supports query parameters to filter the list of models returned.
​
output_modalities
Filter models by their output capabilities. Accepts a comma-separated list of modalities or "all" to include every model regardless of output type.
Value	Description
text	Models that produce text output (default)
image	Models that generate images
audio	Models that produce audio output
embeddings	Embedding models
all	Include all models, skip modality filtering
Examples:

# Default — text models only
curl "https://openrouter.ai/api/v1/models"

# Image generation models only
curl "https://openrouter.ai/api/v1/models?output_modalities=image"

# Text and image models
curl "https://openrouter.ai/api/v1/models?output_modalities=text,image"

# All models regardless of modality
curl "https://openrouter.ai/api/v1/models?output_modalities=all"

The same parameter is available on the /v1/models/count endpoint so that counts stay consistent with list results.
​
supported_parameters
Filter models by the API parameters they support. For example, to find models that support tool calling:

curl "https://openrouter.ai/api/v1/models?supported_parameters=tools"

​
