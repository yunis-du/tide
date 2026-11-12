# Tide

A simple and clean desktop to-do app: organize tasks with groups, support for priority (star rating), due dates, and subtasks, data is stored locally.

## Local storage and recovery

Task data is stored in `~/.tide/data.json`; settings use the platform's Tide configuration directory. Saves run through one ordered writer and replace files atomically. Each file keeps its previous valid contents in a sibling `.bak` file. Pending writes are drained before quitting.

If a primary file is damaged or missing, Tide restores a valid backup automatically and shows a recovery notice. Damaged originals are preserved as `<filename>.corrupt-*`. If neither copy can be loaded, Tide stops startup and displays the affected paths instead of replacing existing data with defaults. Failed saves remain visible in the app and can be retried.

## AI settings

The save indicator reflects completed disk writes; failed saves keep the form marked as unsaved. Connection tests use the same streaming request path as task generation.

Thinking controls send provider-specific parameters for supported models, including GPT-5.1, DeepSeek, MiMo, supported Claude 3.7/4–4.6 models, Gemini 2.5 Flash, and Ollama Qwen3. The control is disabled with an explanation for models Tide cannot toggle. Chat Completions and Responses mode are honored for both OpenAI and OpenAI Compatible providers.
