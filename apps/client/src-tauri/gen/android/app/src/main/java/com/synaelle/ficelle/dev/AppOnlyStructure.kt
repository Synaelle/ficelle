package com.synaelle.ficelle.dev

import android.annotation.SuppressLint
import android.credentials.GetCredentialException
import android.credentials.GetCredentialRequest
import android.credentials.GetCredentialResponse
import android.graphics.Matrix
import android.os.Bundle
import android.os.LocaleList
import android.os.OutcomeReceiver
import android.view.ViewStructure
import android.view.autofill.AutofillId
import android.view.autofill.AutofillValue

/**
 * The web view's autofill structure without its web domain. The page reports
 * the web view's internal tauri.localhost, on the form and on every field,
 * and password managers would save and look up logins under that name. With
 * no domain they use the app instead. Everything else passes through, and
 * the children it makes are wrapped the same way. Used from the web view
 * class wry generates, see WRY_RUSTWEBVIEW_CLASS_EXTENSION in
 * .cargo/config.toml.
 */
// the web view only calls what the running Android has
@SuppressLint("NewApi")
class AppOnlyStructure(private val inner: ViewStructure) : ViewStructure() {
  override fun setWebDomain(domain: String?) {}

  override fun newChild(index: Int): ViewStructure = AppOnlyStructure(inner.newChild(index))
  override fun asyncNewChild(index: Int): ViewStructure =
    AppOnlyStructure(inner.asyncNewChild(index))

  override fun addChildCount(num: Int) = inner.addChildCount(num)
  override fun asyncCommit() = inner.asyncCommit()
  override fun clearCredentialManagerRequest() = inner.clearCredentialManagerRequest()
  override fun getAutofillId(): AutofillId? = inner.autofillId
  override fun getChildCount() = inner.childCount
  override fun getExtras(): Bundle = inner.extras
  override fun getHint(): CharSequence? = inner.hint
  override fun getPendingCredentialCallback():
    OutcomeReceiver<GetCredentialResponse, GetCredentialException>? =
    inner.pendingCredentialCallback
  override fun getPendingCredentialRequest(): GetCredentialRequest? =
    inner.pendingCredentialRequest
  override fun getText(): CharSequence? = inner.text
  override fun getTextSelectionEnd() = inner.textSelectionEnd
  override fun getTextSelectionStart() = inner.textSelectionStart
  override fun hasExtras() = inner.hasExtras()
  override fun newHtmlInfoBuilder(tagName: String): HtmlInfo.Builder =
    inner.newHtmlInfoBuilder(tagName)
  override fun setAccessibilityFocused(state: Boolean) = inner.setAccessibilityFocused(state)
  override fun setActivated(state: Boolean) = inner.setActivated(state)
  override fun setAlpha(alpha: Float) = inner.setAlpha(alpha)
  override fun setAutofillHints(hint: Array<out String>?) = inner.setAutofillHints(hint)
  override fun setAutofillId(id: AutofillId) = inner.setAutofillId(id)
  override fun setAutofillId(parentId: AutofillId, virtualId: Int) =
    inner.setAutofillId(parentId, virtualId)
  override fun setAutofillOptions(options: Array<out CharSequence>?) =
    inner.setAutofillOptions(options)
  override fun setAutofillType(type: Int) = inner.setAutofillType(type)
  override fun setAutofillValue(value: AutofillValue?) = inner.setAutofillValue(value)
  override fun setCheckable(state: Boolean) = inner.setCheckable(state)
  override fun setChecked(state: Boolean) = inner.setChecked(state)
  override fun setChildCount(num: Int) = inner.setChildCount(num)
  override fun setClassName(className: String?) = inner.setClassName(className)
  override fun setClickable(state: Boolean) = inner.setClickable(state)
  override fun setContentDescription(contentDescription: CharSequence?) =
    inner.setContentDescription(contentDescription)
  override fun setContextClickable(state: Boolean) = inner.setContextClickable(state)
  override fun setDataIsSensitive(sensitive: Boolean) = inner.setDataIsSensitive(sensitive)
  override fun setDimens(left: Int, top: Int, scrollX: Int, scrollY: Int, width: Int, height: Int) =
    inner.setDimens(left, top, scrollX, scrollY, width, height)
  override fun setElevation(elevation: Float) = inner.setElevation(elevation)
  override fun setEnabled(state: Boolean) = inner.setEnabled(state)
  override fun setFocusable(state: Boolean) = inner.setFocusable(state)
  override fun setFocused(state: Boolean) = inner.setFocused(state)
  override fun setHint(hint: CharSequence?) = inner.setHint(hint)
  override fun setHintIdEntry(entryName: String) = inner.setHintIdEntry(entryName)
  override fun setHtmlInfo(htmlInfo: HtmlInfo) = inner.setHtmlInfo(htmlInfo)
  override fun setId(id: Int, packageName: String?, typeName: String?, entryName: String?) =
    inner.setId(id, packageName, typeName, entryName)
  override fun setImportantForAutofill(mode: Int) = inner.setImportantForAutofill(mode)
  override fun setInputType(inputType: Int) = inner.setInputType(inputType)
  override fun setLocaleList(localeList: LocaleList?) = inner.setLocaleList(localeList)
  override fun setLongClickable(state: Boolean) = inner.setLongClickable(state)
  override fun setMaxTextEms(maxEms: Int) = inner.setMaxTextEms(maxEms)
  override fun setMaxTextLength(maxLength: Int) = inner.setMaxTextLength(maxLength)
  override fun setMinTextEms(minEms: Int) = inner.setMinTextEms(minEms)
  override fun setOpaque(opaque: Boolean) = inner.setOpaque(opaque)
  override fun setPendingCredentialRequest(
    request: GetCredentialRequest,
    callback: OutcomeReceiver<GetCredentialResponse, GetCredentialException>,
  ) = inner.setPendingCredentialRequest(request, callback)
  override fun setReceiveContentMimeTypes(mimeTypes: Array<out String>?) =
    inner.setReceiveContentMimeTypes(mimeTypes)
  override fun setSelected(state: Boolean) = inner.setSelected(state)
  override fun setText(text: CharSequence?) = inner.setText(text)
  override fun setText(text: CharSequence?, selectionStart: Int, selectionEnd: Int) =
    inner.setText(text, selectionStart, selectionEnd)
  override fun setTextIdEntry(entryName: String) = inner.setTextIdEntry(entryName)
  override fun setTextLines(charOffsets: IntArray?, baselines: IntArray?) =
    inner.setTextLines(charOffsets, baselines)
  override fun setTextStyle(size: Float, fgColor: Int, bgColor: Int, style: Int) =
    inner.setTextStyle(size, fgColor, bgColor, style)
  override fun setTransformation(matrix: Matrix?) = inner.setTransformation(matrix)
  override fun setVisibility(visibility: Int) = inner.setVisibility(visibility)
}
