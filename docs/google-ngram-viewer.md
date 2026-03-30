Google Ngram Viewer  [ Books Ngram Viewer ](https://books.google.com/ngrams)


## Release Notes


#### July 2024


### New dataset available

A new Ngram Viewer dataset has been added. When new datasets are released, they contain up-to-date words and phrases, so charts for some ngrams may change. Charts also continue to change due to improvements in OCR and language detection, and as books are added to our corpus.

Previous datasets (2009, 2012, 2019) have been removed from the corpus pulldown, but are still available and can be accessed using search operators
(`:eng_2019` , `:fre_2012`, etc.). You can continue to use URLs that point to older datasets.


### Support for more words in Ngram Viewer search

You can now include up to 7 words in your Ngram Viewer search query (previously limited to 5). Part-of-speech support is still limited to 1-5 tokens per query.


## What does the Ngram Viewer do?

When you enter phrases into the Google Books Ngram Viewer, it displays a graph showing how those phrases have occurred in a corpus of books (e.g., "British English", "English Fiction", "French") over the selected years. Let's look at a sample graph:

  1950 2000(click on line/label for focus)  0.000000% 0.001000% Ngrams chart for nursery school,kindergarten,child care  nursery schoolkindergartenchild care

This shows trends in three ngrams from 1960 to 2015: "nursery school" (a *2-gram* or *bigram* ), "kindergarten" (a *1-gram* or *unigram*), and "child care" (another bigram). What the y-axis shows is this: of all the bigrams contained in our sample of books written in English and published in the United States, what percentage of them are "nursery school" or "child care"? Of all the unigrams, what percentage of them are "kindergarten"? Here, you can see that use of the phrase "child care" started to rise in the late 1960s, overtaking "nursery school" around 1970 and then "kindergarten" around 1973. It peaked shortly after 1990 and has been falling steadily since.

(Interestingly, the results are noticeably different when the corpus is switched to British English.)

You can hover over the line plot for an ngram, which highlights it. With a left-click on a line plot, you can focus on a particular ngram, greying out the other ngrams in the chart, if any. On subsequent left clicks on other line plots in the chart, multiple ngrams can be focused on. You can double click on any area of the chart to reinstate all the ngrams in the query.

You can also specify wildcards in queries, search for inflections, perform case insensitive search, look for particular parts of speech, or add, subtract, and divide ngrams. More on those under [Advanced Usage](https://books.google.com/ngrams/info#advanced)


## Advanced Usage

A few features of the Ngram Viewer may appeal to users who want to dig a little deeper into phrase usage: *wildcard search* , *inflection search* , *case insensitive search* , *part-of-speech tags* and *ngram compositions*.


#### Wildcard search

When you put a `*` in place of a word, the Ngram Viewer will display the top ten substitutions. For instance, to find the most popular words following "University of", search for " [University of \*.](https://books.google.com/ngrams/graph?content=University+of+*&year_start=1800&year_end=2019&corpus=en&smoothing=3)"

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% 0.00050% 0.00100% 0.00150% 0.00200% Ngrams chart for University of California,University of Chicago,University of Michigan,University of Illinois,University of Pennsylvania,University of Wisconsin,University of Minnesota,University of New,University of Texas,University of North  University of CaliforniaUniversity of ChicagoUniversity of MichiganUniversity of IllinoisUniversity of PennsylvaniaUniversity of WisconsinUniversity of MinnesotaUniversity of NewUniversity of TexasUniversity of North

You can right click on any of the replacement ngrams to collapse them all into the original wildcard query, with the result being the yearwise sum of the replacements. A subsequent right click expands the wildcard query back to all the replacements. Note that the Ngram Viewer only supports one \* per ngram.

Note that the top ten replacements are computed for the specified time range. You might therefore get different replacements for different year ranges. We've filtered punctuation symbols from the top ten list, but for words that often start or end sentences, you might see one of the sentence boundary symbols (`_START_` or `_END_`) as one of the replacements.


#### Inflection search

An *inflection* is the modification of a word to represent various grammatical categories such as aspect, case, gender, mood, number, person, tense and voice. You can search for them by appending \_INF to an ngram. For instance, searching " [book`_INF` a hotel](https://books.google.com/ngrams/graph?content=book_INF+a+hotel&year_start=1800&year_end=2022&corpus=en&smoothing=3)" will display results for "book", "booked", "books", and "booking":

  2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000000% 0.00000200% Ngrams chart for book a hotel,booked a hotel,booking a hotel,books a hotel  book a hotelbooked a hotelbooking a hotelbooks a hotel

Right clicking any inflection collapses all forms into their sum. Note that the Ngram Viewer only supports one `_INF` keyword per query.

**Warning:** You can't freely mix wildcard searches, inflections and case-insensitive searches for one particular ngram. However, you can search with either of these features for separate ngrams in a query: "book`_INF` a hotel, book \* hotel" is fine, but "book`_INF` \* hotel" is not.


#### Case insensitive search

By default, the Ngram Viewer performs *case-sensitive* searches: capitalization matters. You can perform a case-insensitive search by selecting the "case-insensitive" checkbox to the right of the query box. The Ngram Viewer will then display the yearwise sum of the most common case-insensitive variants of the input query. Here are two case-insensitive ngrams, ["Fitzgerald" and "Dupont"](https://books.google.com/ngrams/graph?content=Fitzgerald%2CDupont&year_start=1800&year_end=2022&corpus=en&smoothing=3&case_insensitive=true):

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.000000% 0.000500% Ngrams chart for Fitzgerald (All),Dupont (All)  Fitzgerald (All)Dupont (All)

Right clicking any yearwise sum results in an expansion into the most common case-insensitive variants. For example, a right click on "Dupont (All)" results in the following four variants: "DuPont", "Dupont", "duPont" and "DUPONT".


#### Part-of-speech Tags

Consider the word *tackle* , which can be a verb ("tackle the problem") or a noun ("fishing tackle"). You can distinguish between these different forms by appending `_VERB` or `_NOUN` , example: " [tackle`_VERB` , tackle`_NOUN`](https://books.google.com/ngrams/graph?content=tackle_VERB%2 Ctackle_NOUN&year_start=1800&year_end=2022&corpus=en&smoothing=3)"

  1800 1900 2000(click on line/label for focus)  0.000000% 0.000500% Ngrams chart for tackle\_VERB,tackle\_NOUN  tackle\_VERBtackle\_NOUN

The full list of tags is as follows:

| `_NOUN_` | Noun | These tags can either stand alone (`_PRON_`) or can be appended to a word (she\_`PRON`) |
| `_PROPN_` | Proper Noun |
| `_VERB_` | Verb |
| `_ADJ_` | Adjective |
| `_ADV_` | Adverb |
| `_PRON_` | Pronoun |
| `_DET_` | Determiner or article |
| `_ADP_` | An adposition: either a preposition or a postposition |
| `_NUM_` | Numeral |
| `_CONJ_` | Conjunction |
| `_PRT_` | Particle |
| `_ROOT_` | Root of the parse tree | These tags must stand alone (e.g., `_START_` ) |
| `_START_` | Start of a sentence |
| `_END_` | End of a sentence |

Since the part-of-speech tags needn't attach to particular words, you can use the `DET` tag to search for *read a book* , *read the book* , *read that book* , *read this book* , and so on as follows, " [read `_DET_` book](https://books.google.com/ngrams/graph?content=read+_DET_+book&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

  1800 2000(click on line/label for focus)  0.000000% 0.000200% Ngrams chart for read \_DET\_ book  read \_DET\_ book

If you wanted to know what the most common determiners in this context are, you could combine wildcards and part-of-speech tags to [*read* `*_DET` *book*](https://books.google.com/ngrams/graph?content=read+*_DET+book&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.0000000% 0.0000200% 0.0000400% 0.0000600% 0.0000800% Ngrams chart for read the\_DET book,read a\_DET book,read this\_DET book,read that\_DET book,read any\_DET book,read every\_DET book,read no\_DET book,read some\_DET book,read another\_DET book,read The\_DET book  read the\_DET bookread a\_DET bookread this\_DET bookread that\_DET bookread any\_DET bookread every\_DET bookread no\_DET bookread some\_DET bookread another\_DET bookread The\_DET book

To get all the different inflections of the word *book* which have been followed by a `NOUN` in the corpus you can issue the query [*book* `_INF _NOUN_`](https://books.google.com/ngrams/graph?content=book_INF+_NOUN_&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1800 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% 0.00200% Ngrams chart for book \_NOUN\_,books \_NOUN\_,booking \_NOUN\_,booked \_NOUN\_  book \_NOUN\_books \_NOUN\_booking \_NOUN\_booked \_NOUN\_

Most frequent part-of-speech tags for a word can be retrieved with the wildcard functionality. Consider the query [*cooks\_\**](https://books.google.com/ngrams/graph?content=cook_*&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1900 1950 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% Ngrams chart for cook\_NOUN,cook\_VERB  cook\_NOUNcook\_VERB

The inflection keyword can also be combined with part-of-speech tags. For example, consider the query [*cook* `_INF` , *cook* `_VERB_` `INF`](https://books.google.com/ngrams/graph?content=cook_INF%2C+cook_VERB_INF&year_start=1800&year_end=2022&corpus=en&smoothing=3) below, that separates out the inflections of the verbal sense of "cook":

  1900 1950 2000(click on line/label for focus, right click to expand/contract wildcards)  0.00000% 0.00100% 0.00200% Ngrams chart for cooking,cook,cooked,cooks,cooked\_VERB,cook\_VERB,cooking\_VERB,cooks\_VERB  cookingcookcookedcookscooked\_VERBcook\_VERBcooking\_VERBcooks\_VERB

The Ngram Viewer tags sentence boundaries, allowing you to identify ngrams at starts and ends of sentences with the `_START_` and `_END_` tags, example: " [`_START_` President Truman,`_START_` President Lincoln,`_START_` President Harding](https://books.google.com/ngrams/graph?content=_START_+President+Truman%2C+_START_+President+Lincoln%2C+_START_+President+Harding&year_start=1800&year_end=2022&corpus=en&smoothing=3)"

  2000(click on line/label for focus)  0.000000% 0.000100% Ngrams chart for \_START\_ President Truman,\_START\_ President Lincoln,\_START\_ President Harding  \_START\_ President Truman\_START\_ President Lincoln\_START\_ President Harding

Sometimes it helps to think about words in terms of dependencies rather than patterns. Let's say you want to know how often *tasty* modifies *dessert* . That is, you want to tally mentions of *tasty frozen dessert* , *crunchy, tasty dessert* , *tasty yet expensive dessert* , and all the other instances in which the word *tasty* is applied to *dessert* . For that, the Ngram Viewer provides dependency relations with the `=>` operator [dessert`=>`tasty](https://books.google.com/ngrams/graph?content=dessert%3D%3 Etasty&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1900 2000(click on line/label for focus)  0.000000000% 0.000000500% Ngrams chart for dessert=>tasty  dessert=>tasty

Every parsed sentence has a `_ROOT_` . Unlike other tags, `_ROOT_` doesn't stand for a particular word or position in the sentence. It's the root of the parse tree constructed by analyzing the syntax; you can think of it as a placeholder for what the main verb of the sentence is modifying. So here's how to identify how often *will* was the main verb of a sentence, " [`_ROOT_` `=>`will"](https://books.google.com/ngrams/graph?content=_ROOT_%3D%3 Ewill&year_start=1800&year_end=20122&corpus=en&smoothing=3):

  1800 1900 2000(click on line/label for focus)  0.00000% Ngrams chart for \_ROOT\_=>will  \_ROOT\_=>will

The above graph would include the sentence *Larry will decide.* but not *Larry said that he will decide* , since *will* isn't the main verb of that sentence.

Dependencies can be combined with wildcards. For example, consider the query [*drink=>\** `_NOUN`](https://books.google.com/ngrams/graph?content=drink%3D%3E*_NOUN&year_start=1800&year_end=2022&corpus=en&smoothing=3) below:

  1900 2000(click on line/label for focus, right click to expand/contract wildcards)  0.0000000% 0.0000200% 0.0000400% 0.0000600% 0.0000800% 0.0001000% Ngrams chart for drink=>water\_NOUN,drink=>wine\_NOUN,drink=>milk\_NOUN,drink=>tea\_NOUN,drink=>beer\_NOUN,drink=>coffee\_NOUN,drink=>cup\_NOUN,drink=>blood\_NOUN,drink=>glass\_NOUN,drink=>health\_NOUN  drink=>water\_NOUNdrink=>wine\_NOUNdrink=>milk\_NOUNdrink=>tea\_NOUNdrink=>beer\_NOUNdrink=>coffee\_NOUNdrink=>cup\_NOUNdrink=>blood\_NOUNdrink=>glass\_NOUNdrink=>health\_NOUN

"Pure" part-of-speech tags can be mixed freely with regular words in 1-, 2-, 3-, 4-, and 5-grams (e.g., `the _ADJ_ toast` or `_DET_ _ADJ_ toast` ). " [`_DET_` bright`_ADJ` rainbow](https://books.google.com/ngrams/graph?content=_DET_+bright_ADJ+rainbow&year_start=1800&year_end=2022&corpus=en&smoothing=3)"

  1800 2000(click on line/label for focus)  0.00000000% 0.00000200% Ngrams chart for \_DET\_ bright\_ADJ rainbow  \_DET\_ bright\_ADJ rainbow


#### Ngram Compositions

The Ngram Viewer provides five operators that you can use to combine ngrams: `+` , `-` , `/` , `*` , and `:`.

| `+` | Sums the expressions on either side, letting you combine multiple ngram time series into one. |
| `-` | Subtracts the expression on the right from the expression on the left, giving you a way to measure one ngram relative to another. Because users often want to search for hyphenated phrases, put spaces on either side of the `-` sign. |
| `/` | Divides the expression on the left by the expression on the right, which is useful for isolating the behavior of an ngram with respect to another. |
| `*` | Multiplies the expression on the left by the number on the right, making it easier to compare ngrams of very different frequencies. (Be sure to enclose the entire ngram in parentheses so that \* isn't interpreted as a wildcard.) |
| `:` | Applies the ngram on the left to the corpus on the right, allowing you to compare ngrams across different corpora. |

The Ngram Viewer will try to guess whether to apply these behaviors. You can use parentheses to force them on, and square brackets to force them off. Example: `and/or` will divide *and* by *or* ; to measure the usage of the phrase *and/or* , use `[and/or]` . And `well-meaning` will search for the phrase *well-meaning* ; if you want to subtract meaning from well, use `(well - meaning)`.

To demonstrate the `+` operator, here's how you might find the sum of *game* , *sport* , and *play* , [game, sport ,play, (game + sport + play)](https://books.google.com/ngrams/graph?content=game%2C+sport%2C+play%2C+%28game+%2B+sport+%2B+play%29&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  1800 2000(click on line/label for focus)  0.0000% 0.0200% Ngrams chart for game,sport,play,(game + sport + play)  gamesportplay(game + sport + play)

When determining whether people wrote more about choices over the years, you could compare *choice* , *selection* , *option* , and *alternative* , specifying the noun forms to avoid the adjective forms (e.g., *choice delicacy* , *alternative music* ), example " [(choice`_NOUN` + selection`_NOUN` + option`_NOUN` + alternative`_NOUN`)](https://books.google.com/ngrams/graph?content=%28choice_NOUN+%2B+selection_NOUN+%2B+option_NOUN+%2B+alternative_NOUN%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0000% 0.0200% Ngrams chart for (choice\_NOUN + selection\_NOUN + option\_NOUN + alternative\_NOUN)  (choice\_NOUN + selection\_NOUN + option\_NOUN + alternative\_NOUN)

Ngram subtraction gives you an easy way to compare one set of ngrams to another, " [((Bigfoot + Sasquatch) - (Loch Ness monster + Nessie))](https://books.google.com/ngrams/graph?content=%28choice_NOUN+%2B+selection_NOUN+%2B+option_NOUN+%2B+alternative_NOUN%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0000000% Ngrams chart for ((Bigfoot + Sasquatch) - (Loch Ness monster + Nessie))  ((Bigfoot + Sasquatch) - (Loch Ness monster + Nessie))

Here's how you might combine `+` and `/` to show how the word *applesauce* has blossomed at the expense of *apple sauce* , " [(applesauce / (apple sauce + applesauce))](https://books.google.com/ngrams/graph?content=%28applesauce+%2F+%28apple+sauce+%2B+applesauce%29%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0% 100.0% Ngrams chart for (applesauce / (apple sauce + applesauce))  (applesauce / (apple sauce + applesauce))

The `*` operator is useful when you want to compare ngrams of widely varying frequencies, like *violin* and the more esoteric *theremin* , " [(theremin \* 1000),violin"](https://books.google.com/ngrams/graph?content=%28theremin+*+1000%29%2C+violin&year_start=1800&year_end=2022&corpus=en&smoothing=3):

  2000(click on line/label for focus)  0.00000% Ngrams chart for (theremin \* 1000),violin  (theremin \* 1000)violin

The `:corpus` selection operator lets you compare ngrams in different languages, or American versus British English (or fiction), or between the 2009, 2012, 2019, and the current versions of our book scans. Here's *chat* in English versus the same unigram in French, " [chat:fre, chat:eng](https://books.google.com/ngrams/graph?content=chat%3 Afre%2C+chat%3 Aeng&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

  1980 2000(click on line/label for focus)  0.00000% 0.00200% Ngrams chart for (chat:eng),(chat:fre)  (chat:eng)(chat:fre)

When we generated the original Ngram Viewer corpora in 2009, our OCR wasn't as good as it is today. This was especially obvious in pre-19th century English, where the elongated medial-s (ſ) was often interpreted as an *f* , so *best* was often read as *beft* . Here's evidence of the improvements we've made since then, using the corpus operator to compare the 2009, 2012, 2019 and Current versions, " [beft:eng,beft:eng\_2019,beft:eng\_2012,beft:eng\_2009](https://books.google.com/ngrams/graph?content=beft%3 Aeng_2019%2 Cbeft%3 Aeng_2012%2 Cbeft%3 Aeng_2009&year_start=1600&year_end=2015&corpus=en&smoothing=3)":

  2000(click on line/label for focus)  0.0000% Ngrams chart for (beft:eng\_2009),(beft:eng\_2012),(beft:eng\_2019)  (beft:eng\_2009)(beft:eng\_2012)(beft:eng\_2019)

By comparing fiction against all of English, we can see that uses of *wizard* in general English have been gaining recently compared to uses in fiction, " [(wizard:eng / wizard:eng\_fiction)](https://books.google.com/ngrams/graph?content=%28wizard%3 Aeng+%2F+wizard%3 Aeng_fiction%29&year_start=1800&year_end=2022&corpus=en&smoothing=3)":

 (click on line/label for focus)  0.0% 50.0% Ngrams chart for (wizard:eng\_2019 / wizard:eng\_fiction\_2019)  (wizard:eng\_2019 / wizard:eng\_fiction\_2019)


## Corpora

Below are descriptions of the corpora that can be searched with the Google Books Ngram Viewer. All corpora were generated in July 2009, July 2012, and February 2020; we will update these corpora as our book scanning continues, and the updated versions will have distinct persistent identifiers. Books with low OCR quality and serials were excluded.

| **Informal corpus name** | **Shorthand** | **Persistent identifier** | **Description** |
| American English | eng\_us |  | Books predominantly in the English language that were published in the United States. |
| American English 2019 | eng\_us\_2019 | googlebooks-eng-us-20200217 |
| American English 2012 | eng\_us\_2012 | googlebooks-eng-us-all-20120701 |
| American English 2009 | eng\_us\_2009 | googlebooks-eng-us-all-20090715 |
| British English | eng\_gb |  | Books predominantly in the English language that were published in Great Britain. |
| British English 2019 | eng\_gb\_2019 | googlebooks-eng-gb-20200217 |
| British English 2012 | eng\_gb\_2012 | googlebooks-eng-gb-all-20120701 |
| British English 2009 | eng\_gb\_2009 | googlebooks-eng-gb-all-20090715 |
| English | eng |  | Books predominantly in the English language published in any country. |
| English 2019 | eng\_2019 | googlebooks-eng-20200217 |
| English 2012 | eng\_2012 | googlebooks-eng-all-20120701 |
| English 2009 | eng\_2009 | googlebooks-eng-all-20090715 |
| English Fiction | eng\_fiction |  | Books predominantly in the English language that a library or publisher identified as fiction. |
| English Fiction 2019 | eng\_fiction\_2019 | googlebooks-eng-fiction-20200217 |
| English Fiction 2012 | eng\_fiction\_2012 | googlebooks-eng-fiction-all-20120701 |
| English Fiction 2009 | eng\_fiction\_2009 | googlebooks-eng-fiction-all-20090715 |
| English One Million | eng\_1m\_2009 | googlebooks-eng-1M-20090715 | The "Google Million". All are in English with dates ranging from 1500 to 2008. No more than about 6000 books were chosen from any one year, which means that all of the scanned books from early years are present, and books from later years are randomly sampled. The random samplings reflect the subject distributions for the year (so there are more computer books in 2000 than 1980). |
| Chinese | chi\_sim |  | Books predominantly in simplified Chinese. |
| Chinese 2019 | chi\_sim\_2019 | googlebooks-chi-sim-20200217 |
| Chinese 2012 | chi\_sim\_2012 | googlebooks-chi-sim-all-20120701 |
| Chinese 2009 | chi\_sim\_2009 | googlebooks-chi-sim-all-20090715 |
| French 2019 | fre |  | Books predominantly in the French language. |
| French 2019 | fre\_2019 | googlebooks-fre-20200217 |
| French 2012 | fre\_2012 | googlebooks-fre-all-20120701 |
| French 2009 | fre\_2009 | googlebooks-fre-all-20090715 |
| German | ger |  | Books predominantly in the German language. |
| German 2019 | ger\_2019 | googlebooks-ger-20200217 |
| German 2012 | ger\_2012 | googlebooks-ger-all-20120701 |
| German 2009 | ger\_2009 | googlebooks-ger-all-20090715 |
| Hebrew | heb |  | Books predominantly in the Hebrew language. |
| Hebrew 2019 | heb\_2019 | googlebooks-heb-20200217 |
| Hebrew 2012 | heb\_2012 | googlebooks-heb-all-20120701 |
| Hebrew 2009 | heb\_2009 | googlebooks-heb-all-20090715 |
| Spanish | spa |  | Books predominantly in the Spanish language. |
| Spanish 2019 | spa\_2019 | googlebooks-spa-20200217 |
| Spanish 2012 | spa\_2012 | googlebooks-spa-all-20120701 |
| Spanish 2009 | spa\_2009 | googlebooks-spa-all-20090715 |
| Russian | rus |  | Books predominantly in the Russian language. |
| Russian 2019 | rus\_2019 | googlebooks-rus-20200217 |
| Russian 2012 | rus\_2012 | googlebooks-rus-all-20120701 |
| Russian 2009 | rus\_2009 | googlebooks-rus-all-20090715 |
| Italian | ita |  | Books predominantly in the Italian language. |
| Italian 2019 | ita\_2019 | googlebooks-ita-20200217 |
| Italian 2012 | ita\_2012 | googlebooks-ita-all-20120701 |

Compared to the 2009 versions, the 2012, 2019, and quarterly released versions have more books, improved OCR, improved library and publisher metadata. The 2012 and 2019 versions also don't form ngrams that cross sentence boundaries, and do form ngrams across page boundaries, unlike the 2009 versions.

With the most recent corpora, the tokenization has improved as well, using a set of manually devised rules (except for Chinese, where a statistical system is used for segmentation). In the 2009 corpora, tokenization was based simply on whitespace.


## Searching inside Google Books

Below the graph, we show "interesting" year ranges for your query terms. Clicking on those will submit your query directly to Google Books. Note that the Ngram Viewer is case-sensitive, but Google Books search results are *not*.

Those searches will yield phrases in the language of whichever corpus you selected, but the results are returned from the full Google Books corpus. So if you use the Ngram Viewer to search for a French phrase in the French corpus and then click through to Google Books, that search will be for the same French phrase -- which might occur in a book predominantly in another language.


## FAQs


#### Why am I not seeing the results I expect?

> Perhaps for one of these reasons:
>
> *  The Ngram Viewer is case-sensitive. Try capitalizing your query or check the "case-insensitive" box to the right of the search box.
> *  You're searching in an unexpected corpus. For instance, *Frankenstein* doesn't appear in Russian books, so if you search in the Russian corpus you'll see a flatline. You can choose the corpus via the dropdown menu below the search box, or through the corpus selection operator, e.g., `Frankenstein:eng_2019`.
> *  Your phrase has a comma, plus sign, hyphen, asterisk, colon, or forward slash in it. Those have special meanings to the Ngram Viewer; see [Advanced Usage](https://books.google.com/ngrams/info#advanced). Try enclosing the phrase in square brackets (although this won't help with commas).


#### How does the Ngram Viewer handle punctuation?

> We apply a set of tokenization rules specific to the particular language. In English, contractions become two words (`they're` becomes the bigram `they 're` , `we'll` becomes `we 'll` , and so on). The possessive `'s` is also split off, but `R'n'B` remains one token. Negations (`n't` ) are normalized so that `don't` becomes `do not` . In Russian, the diacritic ё is normalized to e, and so on. The same rules are applied to parse both the ngrams typed by users and the ngrams extracted from the corpora, which means that if you're searching for `don't` , don't be alarmed by the fact that the Ngram Viewer rewrites it to `do not` ; it is accurately depicting usages of both `don't` and `do not` in the corpus. However, this means there is no way to search explicitly for the specific forms `can't` (or `cannot` ): you get `can't` and `can not` and `cannot` all at once.


#### How can I see sample usages in context?

> Below the Ngram Viewer chart, we provide a table of predefined Google Books searches, each narrowed to a range of years. We choose the ranges according to interestingness: if an ngram has a huge peak in a particular year, that will appear by itself as a search, with other searches covering longer durations.
>
> Unlike the Ngram Viewer corpus, the Google Books corpus isn't part-of-speech tagged. One can't search for, say, the verb form of *cheer* in Google Books. So any ngrams with part-of-speech tags (e.g., `cheer_VERB`) are excluded from the table of Google Books searches.
>
> The Ngram Viewer has 2009, 2012, 2019 and a quarterly released corpora, but Google Books doesn't work that way. When you're searching in Google Books, you're searching all the currently available books, so there may be some differences between what you see in Google Books and what you would expect to see given the Ngram Viewer chart.


#### Why do I see more spikes and plateaus in early years?

> Publishing was a relatively rare event in the 16th and 17th centuries. (There are only [about 500,000 books](http://estc.bl.uk/) published in English before the 19th century.) So if a phrase occurs in one book in one year but not in the preceding or following years, that creates a taller spike than it would in later years.
>
> Plateaus are usually simply smoothed spikes. Change the smoothing to 0.


#### What does "smoothing" mean?

> Often trends become more apparent when data is viewed as a moving average. A smoothing of 1 means that the data shown for 1950 will be an average of the raw count for 1950 plus 1 value on either side: ("count for 1949" + "count for 1950" + "count for 1951"), divided by 3. So a smoothing of 10 means that 21 values will be averaged: 10 on either side, plus the target value in the center of them.
>
> At the left and right edges of the graph, fewer values are averaged. With a smoothing of 3, the leftmost value (pretend it's the year 1950) will be calculated as ("count for 1950" + "count for 1951" + "count for 1952" + "count for 1953"), divided by 4.
>
> A smoothing of 0 means no smoothing at all: just raw data.


#### Many more books are published in modern years. Doesn't this skew the results?

> It would if we didn't normalize by the number of books published in each year.


#### Why are you showing a 0% flatline when I know the phrase in my query occurred in at least one book?

> Under heavy load, the Ngram Viewer will sometimes return a flatline; reload to confirm that there are actually no hits for the phrase. Also, we only consider ngrams that occur in at least 40 books. Otherwise the dataset would balloon in size and we wouldn't be able to [offer them all](https://books.google.com/ngrams/datasets).


#### How accurate is the part-of-speech tagging?

> The part-of-speech tags and dependency relations are predicted automatically. Assessing the accuracy of these predictions is difficult, but for modern English we expect the accuracy of the part-of-speech tags to be around 95% and the accuracy of dependency relations around 85%. On older English text and for other languages the accuracies are lower, but likely above 90% for part-of-speech tags and above 75% for dependencies. This implies a significant number of errors, which should be taken into account when drawing conclusions.
>
> The part-of-speech tags are constructed from a small training set (a mere million words for English). This will sometimes underrepresent uncommon usages, such as *green* or *dog* or *book* as verbs, or *ask* as a noun.
>
> An additional note on Chinese: Before the 20th century, classical Chinese was traditionally used for all written communication. Classical Chinese is based on the grammar and vocabulary of ancient Chinese, and the syntactic annotations will therefore be wrong more often than they're right.
>
> Also, note that the 2009 corpora have not been part-of-speech tagged.


#### I'm writing a paper based on your results. How can I cite your work?

> If you're going to use this data for an academic publication, please cite the original paper:
>
> Jean-Baptiste Michel\*, Yuan Kui Shen, Aviva Presser Aiden, Adrian Veres, Matthew K. Gray, William Brockman, The Google Books Team, Joseph P. Pickett, Dale Hoiberg, Dan Clancy, Peter Norvig, Jon Orwant, Steven Pinker, Martin A. Nowak, and Erez Lieberman Aiden\*. *Quantitative Analysis of Culture Using Millions of Digitized Books* . **Science** (Published online ahead of print: 12/16/2010)
>
> We also have a paper on our part-of-speech tagging:
>
> Yuri Lin, Jean-Baptiste Michel, Erez Lieberman Aiden, Jon Orwant, William Brockman, Slav Petrov. *Syntactic Annotations for the Google Books Ngram Corpus* . **Proceedings of the 50th Annual Meeting of the Association for Computational Linguistics** Volume 2: Demo Papers (ACL '12) (2012)


#### Can I download your data to run my own experiments?

> Yes! The ngram data is available for download [here](https://books.google.com/ngrams/datasets). To make the file sizes manageable, we've grouped them by their starting letter and then grouped the different ngram sizes in separate files. The ngrams within each file are not alphabetically sorted.
>
> To generate machine-readable filenames, we transliterated the ngrams for languages that use non-roman scripts (Chinese, Hebrew, Russian) and used the starting letter of the transliterated ngram to determine the filename. The same approach was taken for characters such as *ä* in German. Note that the transliteration was used only to determine the filename; the actual ngrams are encoded in UTF-8 using the language-specific alphabet.


#### I'd like to publish an Ngram graph in my book/magazine/blog/presentation. What are your licensing terms?

> Ngram Viewer graphs and data may be freely used for any purpose, although acknowledgement of Google Books Ngram Viewer as the source, and inclusion of a link to [https://books.google.com/ngrams](https://books.google.com/ngrams), would be appreciated.

Enjoy!

The Google Ngram Viewer Team, part of [Google Books](https://books.google.com/)

[About Ngram Viewer](https://books.google.com/ngrams/info) [About Google Books](http://www.google.com/intl/en/googlebooks/about.html)  [About Google](http://www.google.com/about.html) [Privacy & Terms](http://www.google.com/intl/en/policies/)

