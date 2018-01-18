'use strict';

function showShortUrl(shortUrl) {
  console.log(shortUrl + '!!!!!');
}

function shorten(url) {
  console.info(`Sending request to shorten ${url}`);
	const request = new XMLHttpRequest();
	request.onreadystatechange = function() {
		if (request.readyState === XMLHttpRequest.DONE) {
			if (request.status === 200) {
				const response = JSON.parse(request.responseText);
				console.info(response);
				showShortUrl(response.shortUrl);
			} else {
				console.error(request.statusText);
			}
		}
	};
	request.open('POST', '/shorten/');
  request.setRequestHeader("Content-Type", "application/x-www-form-urlencoded");
	request.send(new FormData(document.querySelector("#form")));
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
