# AGENTS.MD

## Packaging Norm

After code changes that affect the application, do not stop at build/test verification. Produce a runnable package for local validation.

Required workflow:

1. Run the normal validation suite for the change.
2. Run the full release package script from the repository root:

   ```powershell
   powershell -ExecutionPolicy Bypass -File scripts\package_release.ps1
   ```

3. Keep the generated portable zip in:

   ```text
   F:\AI\dn-ocr-translator\installer\OCR-Translator-portable-v0.1.0.zip
   ```

4. Extract that zip into a stable runnable directory:

   ```text
   F:\AI\dn-ocr-translator\installer\OCR-Translator-portable-v0.1.0
   ```

5. Before replacing the extracted directory, stop any running `OCR-Translator` / `ocr-translator` process when possible.

6. If replacing the extracted directory, first verify the resolved path is inside:

   ```text
   F:\AI\dn-ocr-translator\installer
   ```

7. If the stable directory cannot be replaced because an elevated or locked running instance is holding files, extract the zip to:

   ```text
   F:\AI\dn-ocr-translator\installer\OCR-Translator-portable-v0.1.0-latest
   ```

   Then clearly report that the stable directory is blocked and that the `latest` directory is the runnable build for this turn.

8. In the final response, report the runnable executable path, portable zip path, MSI path, and NSIS setup path.

Do not create a git commit unless the user explicitly asks for one.

## Scene Regression Norm

Every image translation scene must have regression coverage before it is treated as done.

Required for each scene:

1. Add or update a focused test case that captures the scene behavior.
2. Keep existing scene tests passing so changes do not regress older scenes.
3. Include layout assertions, not only text assertions. Examples:
   - number of regions / translation units
   - block width and height limits
   - font size consistency
   - token preservation
   - no oversized block covering unrelated UI
4. Include a lightweight performance budget or metric for scene logic when practical. At minimum, record elapsed time for pure layout / grouping tests or keep the algorithm bounded by input line count.
5. If a new scene type is added, document its detector assumptions and fallback behavior in code or tests.

Scene work is not complete until its tests and performance checks pass.
