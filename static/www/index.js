'use strict';

function copyShortUrl() {
  const shortUrl = document.querySelector('#short-url');

  let temporary = document.createElement('input');
  temporary.value = 'http://' + shortUrl.innerHTML;

  document.body.appendChild(temporary);
  temporary.focus();
  temporary.select();
  document.execCommand('copy');
  document.body.removeChild(temporary);

  console.info(`Copied ${temporary.value} to clipboard`);

  const shortUrlComment = document.querySelector('#short-url-comment');
  shortUrlComment.innerHTML = '[COPIED]';
  shortUrlComment.style.display = '';
}

function resetInput() {
  const form = document.querySelector('#form');
  const url = document.querySelector('#url');
  const shortUrlBox = document.querySelector('#short-url-box');
  const button = document.querySelector('button');

  url.value = '';
  form.style.display = '';
  shortUrlBox.style.display = 'none';

  button.className = '';
  button.innerHTML = 'Go';
  button.onclick = () => shorten(url.value);

  resizeToText('');
}

function showShortUrl(response) {
	console.info(response);
  const form = document.querySelector('#form');
  const shortUrlBox = document.querySelector('#short-url-box');
  const shortUrl = document.querySelector('#short-url');
  const button = document.querySelector('button');

  shortUrl.innerHTML = response.shortUrl;

  form.style.display = 'none';
  shortUrlBox.style.display = 'block';

  button.classList.add('back');
  button.innerHTML = '↩';
  button.onclick = resetInput;

  const shortUrlComment = document.querySelector('#short-url-comment');
  shortUrlComment.innerHTML = '[NEW]';
  shortUrlComment.style.display = response.alreadyExisted ? 'none' : '';
}

function shorten(url) {
  url = encodeURIComponent(url);
	console.info(`Sending request to shorten ${url}`);

	const request = new XMLHttpRequest();
	request.onreadystatechange = function() {
		if (request.readyState === XMLHttpRequest.DONE) {
			if (request.status === 200) {
				const response = JSON.parse(request.responseText);
				console.info(`Received response: ${response}`);
				showShortUrl(response);
			} else {
				console.error(request.statusText);
			}
		}
	};

	request.open('POST', '/shorten');
	request.setRequestHeader("Content-Type", "application/x-www-form-urlencoded");
  request.send(`url=${url}`);
}

function css(element, property) {
	return window.getComputedStyle(element, null).getPropertyValue(property);
}

function resizeToText(text) {
	const form = document.querySelector('#form');
	const fontFamily = css(form, 'font-family');
	const fontSize = css(form, 'font-size');

	const canvas = document.createElement('canvas');
	const context = canvas.getContext("2d");
	context.font = `${fontSize} ${fontFamily}`;
	const width = context.measureText(text).width;
	form.style.width = `${width}px`

	return width;
}

const URL_PATTERN = /^(http:\/\/)?(www\.)?goldsborough\.me((\/[/\w,+-]*?)(#[\w,+-]*)?)?$/i;

function updateCorrectnessHint(text) {
	const input = document.querySelector('#url');
	const button = document.querySelector('button');
  const elements = [input, button];
  elements.forEach(e => e.className = '');
  button.disabled = true;
	if (text.length > 0) {
		if (URL_PATTERN.test(text)) {
      elements.forEach(e => e.classList.add('ok'));
      button.disabled = false;
		} else {
			elements.forEach(e => e.classList.add('invalid'));
		}
	}
}

function onInputChange(text) {
  resizeToText(text);
	updateCorrectnessHint(text);
}
